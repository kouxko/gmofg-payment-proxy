//! Cancellation-safe connection shutdown ownership.

use std::sync::Arc;

use intercept_proxy_domain::ProtocolPackageRef;
use tokio::sync::OwnedMutexGuard;

use super::{ExternalPackageRegistryAdapter, OnlineConnection};

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct DisconnectBarrier {
    pub(super) reached: Arc<tokio::sync::Notify>,
    pub(super) release: Arc<tokio::sync::Notify>,
}

impl ExternalPackageRegistryAdapter {
    #[cfg(test)]
    pub(super) fn install_disconnect_barrier(
        &self,
        package: ProtocolPackageRef,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let barrier = DisconnectBarrier {
            reached: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        self.disconnect_barriers
            .lock()
            .insert(package, barrier.clone());
        (barrier.reached, barrier.release)
    }

    pub(super) fn connection_mutation(
        &self,
        package: &ProtocolPackageRef,
    ) -> Arc<tokio::sync::Mutex<()>> {
        self.connection_mutations
            .lock()
            .entry(package.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub(super) async fn begin_disconnect(
        &self,
        package: &ProtocolPackageRef,
        environment_apply_gate: Option<OwnedMutexGuard<()>>,
    ) -> tokio::sync::watch::Receiver<bool> {
        let gate = self.connection_mutation(package);
        let _mutation = gate.lock().await;
        let (connection_id, client, completion, completed) = {
            let mut online = self.online.lock();
            match online.remove(package) {
                Some(OnlineConnection::Active { id, client }) => {
                    let (completed, completion) = tokio::sync::watch::channel(false);
                    online.insert(
                        package.clone(),
                        OnlineConnection::Closing {
                            id,
                            completion: completion.clone(),
                        },
                    );
                    (Some(id), Some(client), completion, Some(completed))
                }
                Some(OnlineConnection::Closing { id, completion }) => {
                    online.insert(
                        package.clone(),
                        OnlineConnection::Closing {
                            id,
                            completion: completion.clone(),
                        },
                    );
                    (Some(id), None, completion, None)
                }
                None => {
                    let (_completed, completion) = tokio::sync::watch::channel(true);
                    (None, None, completion, None)
                }
            }
        };
        if let (Some(connection_id), Some(client), Some(completed)) =
            (connection_id, client, completed)
        {
            let registry = self.clone();
            let package = package.clone();
            tokio::spawn(async move {
                #[cfg(test)]
                let barrier = { registry.disconnect_barriers.lock().remove(&package) };
                #[cfg(test)]
                if let Some(barrier) = barrier {
                    barrier.reached.notify_one();
                    barrier.release.notified().await;
                }
                client.disconnect().await;
                let gate = registry.connection_mutation(&package);
                let _mutation = gate.lock().await;
                let mut online = registry.online.lock();
                let owns_closing = matches!(
                    online.get(&package),
                    Some(OnlineConnection::Closing { id, .. }) if *id == connection_id
                );
                if owns_closing {
                    online.remove(&package);
                }
                drop(online);
                let _ = completed.send(true);
                if owns_closing {
                    registry.publish_catalog_changed(&package);
                    registry.publish_service_status();
                    registry.online_changed.notify_waiters();
                }
                drop(environment_apply_gate);
                #[cfg(test)]
                registry.cleanup_complete.notify_one();
            });
        }
        completion
    }

    pub(super) async fn wait_for_closing(completion: &mut tokio::sync::watch::Receiver<bool>) {
        while !*completion.borrow() {
            if completion.changed().await.is_err() {
                break;
            }
        }
    }
}
