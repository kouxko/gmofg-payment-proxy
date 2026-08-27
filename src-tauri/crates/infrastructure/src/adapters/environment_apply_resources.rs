use std::{collections::BTreeMap, sync::Arc};

use parking_lot::Mutex;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey;

/// One process-wide registry shared by apply leases and every runtime publisher they guard.
#[derive(Debug, Default)]
pub(crate) struct EnvironmentApplyResourceGateRegistry {
    gates: Mutex<BTreeMap<EnvironmentApplyLeaseResourceKey, Arc<AsyncMutex<()>>>>,
    projections: Mutex<ResourceProjections>,
}

#[derive(Debug, Default)]
struct ResourceProjections {
    next_generation: u64,
    service_epoch: u64,
    entries: BTreeMap<EnvironmentApplyLeaseResourceKey, ResourceProjection>,
    exact_packages: BTreeMap<EnvironmentApplyLeaseResourceKey, ExactPackageProjection>,
}

#[derive(Debug)]
struct ResourceProjection {
    fingerprint: Option<String>,
    generation: u64,
    online: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExactPackageProjection {
    pub(crate) service_epoch: u64,
    pub(crate) description_fingerprint: [u8; 32],
    pub(crate) online_generation: u64,
    pub(crate) lease_generation: u64,
}

impl EnvironmentApplyResourceGateRegistry {
    pub(crate) fn reconcile_listener_projections(
        &self,
        observed: impl IntoIterator<Item = (uuid::Uuid, String)>,
    ) -> u64 {
        self.reconcile_projections(
            |key| matches!(key, EnvironmentApplyLeaseResourceKey::Listener(_)),
            observed.into_iter().map(|(listener_id, fingerprint)| {
                (
                    EnvironmentApplyLeaseResourceKey::Listener(listener_id),
                    fingerprint,
                )
            }),
        )
    }

    pub(crate) fn reconcile_android_projections(
        &self,
        observed: impl IntoIterator<Item = (String, String, String)>,
    ) -> u64 {
        let observed = observed
            .into_iter()
            .map(|(profile_id, serial, fingerprint)| {
                (Self::android_owner_key(&profile_id, &serial), fingerprint)
            });
        self.reconcile_projections(
            |key| matches!(key, EnvironmentApplyLeaseResourceKey::AndroidOwner { .. }),
            observed,
        )
    }

    pub(crate) fn reconcile_exact_package_projections(
        &self,
        observed: impl IntoIterator<Item = (intercept_proxy_domain::ProtocolPackageRef, String)>,
    ) -> u64 {
        self.reconcile_projections(
            |key| matches!(key, EnvironmentApplyLeaseResourceKey::ExactPackage(_)),
            observed.into_iter().map(|(package, fingerprint)| {
                (
                    EnvironmentApplyLeaseResourceKey::ExactPackage(package),
                    fingerprint,
                )
            }),
        )
    }

    pub(crate) fn observe_exact_package_projection(
        &self,
        package: &intercept_proxy_domain::ProtocolPackageRef,
        fingerprint: String,
        description_fingerprint: [u8; 32],
        online: bool,
    ) -> ExactPackageProjection {
        let mut projections = self.projections.lock();
        let key = EnvironmentApplyLeaseResourceKey::ExactPackage(package.clone());
        let previous_online = projections
            .entries
            .get(&key)
            .and_then(|projection| projection.online);
        advance_projection(&mut projections, key.clone(), Some(fingerprint));
        let lease_generation = projections
            .entries
            .get(&key)
            .map_or(0, |projection| projection.generation);
        let service_epoch = projections.service_epoch;
        let previous = projections.exact_packages.get(&key).copied();
        let online_generation = match (previous, previous_online) {
            (Some(previous), Some(previous_online)) if previous_online == online => {
                previous.online_generation
            }
            _ => lease_generation,
        };
        projections.exact_packages.insert(
            key.clone(),
            ExactPackageProjection {
                service_epoch,
                description_fingerprint,
                online_generation,
                lease_generation,
            },
        );
        projections
            .entries
            .get_mut(&key)
            .expect("exact package projection was inserted")
            .online = Some(online);
        ExactPackageProjection {
            service_epoch,
            description_fingerprint,
            online_generation,
            lease_generation,
        }
    }

    pub(crate) fn advance_exact_package_service_epoch(&self) {
        let mut projections = self.projections.lock();
        projections.service_epoch = projections
            .service_epoch
            .checked_add(1)
            .expect("external package service epoch exhausted");
    }

    pub(crate) fn publish_listener_projection(
        &self,
        listener_id: uuid::Uuid,
        fingerprint: Option<String>,
    ) -> u64 {
        self.publish_projection(
            &EnvironmentApplyLeaseResourceKey::Listener(listener_id),
            fingerprint,
        )
    }

    pub(crate) fn publish_android_projection(
        &self,
        profile_id: &str,
        serial: &str,
        fingerprint: Option<String>,
    ) -> u64 {
        self.publish_projection(&Self::android_owner_key(profile_id, serial), fingerprint)
    }

    fn publish_projection(
        &self,
        key: &EnvironmentApplyLeaseResourceKey,
        fingerprint: Option<String>,
    ) -> u64 {
        let mut projections = self.projections.lock();
        force_projection(&mut projections, key.clone(), fingerprint);
        projections
            .entries
            .get(key)
            .map_or(0, |projection| projection.generation)
    }

    fn reconcile_projections(
        &self,
        belongs_to_family: impl Fn(&EnvironmentApplyLeaseResourceKey) -> bool,
        observed: impl IntoIterator<Item = (EnvironmentApplyLeaseResourceKey, String)>,
    ) -> u64 {
        let observed = observed.into_iter().collect::<BTreeMap<_, _>>();
        let mut projections = self.projections.lock();
        let existing = projections
            .entries
            .keys()
            .filter(|key| belongs_to_family(key))
            .cloned()
            .collect::<Vec<_>>();
        for key in existing {
            if !observed.contains_key(&key) {
                advance_projection(&mut projections, key, None);
            }
        }
        for (key, fingerprint) in observed {
            advance_projection(&mut projections, key, Some(fingerprint));
        }
        projections
            .entries
            .iter()
            .filter(|(key, _)| belongs_to_family(key))
            .map(|(_, projection)| projection.generation)
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn android_owner_key(
        profile_id: &str,
        serial: &str,
    ) -> EnvironmentApplyLeaseResourceKey {
        EnvironmentApplyLeaseResourceKey::AndroidOwner {
            profile_id: profile_id.to_owned(),
            serial: serial.to_owned(),
        }
    }

    pub(crate) fn leased_android_owner_key_for_device(
        &self,
        serial: &str,
    ) -> Option<EnvironmentApplyLeaseResourceKey> {
        self.gates.lock().iter().find_map(|(key, gate)| match key {
            EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id,
                serial: gated_serial,
            } if gated_serial == serial && gate.try_lock().is_err() => {
                Some(EnvironmentApplyLeaseResourceKey::AndroidOwner {
                    profile_id: profile_id.clone(),
                    serial: gated_serial.clone(),
                })
            }
            _ => None,
        })
    }

    pub(crate) fn gate(&self, key: &EnvironmentApplyLeaseResourceKey) -> Arc<AsyncMutex<()>> {
        Arc::clone(
            self.gates
                .lock()
                .entry(key.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    pub(crate) async fn acquire(
        &self,
        key: EnvironmentApplyLeaseResourceKey,
    ) -> OwnedMutexGuard<()> {
        self.gate(&key).lock_owned().await
    }

    pub(crate) async fn acquire_all(
        &self,
        mut keys: Vec<EnvironmentApplyLeaseResourceKey>,
    ) -> Vec<OwnedMutexGuard<()>> {
        keys.sort_by(canonical_resource_cmp);
        keys.dedup();
        let mut guards = Vec::with_capacity(keys.len());
        for key in keys {
            guards.push(self.acquire(key).await);
        }
        guards
    }

    pub(crate) async fn acquire_known_exact_package_gates(&self) -> Vec<OwnedMutexGuard<()>> {
        let keys = self
            .gates
            .lock()
            .keys()
            .filter(|key| matches!(key, EnvironmentApplyLeaseResourceKey::ExactPackage(_)))
            .cloned()
            .collect();
        self.acquire_all(keys).await
    }

    #[cfg(test)]
    pub(crate) async fn acquire_known_android_owner_gates(&self) -> Vec<OwnedMutexGuard<()>> {
        let keys = self
            .gates
            .lock()
            .keys()
            .filter(|key| matches!(key, EnvironmentApplyLeaseResourceKey::AndroidOwner { .. }))
            .cloned()
            .collect();
        self.acquire_all(keys).await
    }
}

fn advance_projection(
    projections: &mut ResourceProjections,
    key: EnvironmentApplyLeaseResourceKey,
    fingerprint: Option<String>,
) {
    if projections
        .entries
        .get(&key)
        .is_some_and(|current| current.fingerprint == fingerprint)
    {
        return;
    }
    projections.next_generation = projections
        .next_generation
        .checked_add(1)
        .expect("environment apply resource generation exhausted");
    projections.entries.insert(
        key,
        ResourceProjection {
            fingerprint,
            generation: projections.next_generation,
            online: None,
        },
    );
}

fn force_projection(
    projections: &mut ResourceProjections,
    key: EnvironmentApplyLeaseResourceKey,
    fingerprint: Option<String>,
) {
    projections.next_generation = projections
        .next_generation
        .checked_add(1)
        .expect("environment apply resource generation exhausted");
    projections.entries.insert(
        key,
        ResourceProjection {
            fingerprint,
            generation: projections.next_generation,
            online: None,
        },
    );
}

pub(super) fn canonical_resource_cmp(
    left: &EnvironmentApplyLeaseResourceKey,
    right: &EnvironmentApplyLeaseResourceKey,
) -> std::cmp::Ordering {
    super::environment_configuration_lease::resource_key_cmp(left, right)
}
