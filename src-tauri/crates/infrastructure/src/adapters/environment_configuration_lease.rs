use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AndroidControlPort, AndroidRuntimeOwnerState, AndroidRuntimeOwnerViewModel, AppError,
    AppResult, EnvironmentApplyGenerations, EnvironmentApplyLease, EnvironmentApplyLeasePort,
    EnvironmentApplyLeaseRequest, EnvironmentValidatedApplyBaseline, ListenerRuntimePort,
    ListenerRuntimeState,
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

use super::{EnvironmentApplyResourceGateRegistry, ExternalPackageRegistryAdapter};
use crate::SqliteExecutor;

mod package;
mod resource;
pub(crate) use resource::EnvironmentApplyLeaseResourceKey;
pub(super) use resource::resource_key_cmp;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentApplyLeaseResourceObservation {
    Listener {
        runtime_epoch: Option<Uuid>,
        active_count: u32,
    },
    ListenerActive {
        runtime_epoch: Option<Uuid>,
        active_count: u32,
    },
    Android {
        serial: String,
        owner_epoch: Option<Uuid>,
        state: String,
    },
    ExactPackage {
        generation: Uuid,
        enabled: bool,
        online: bool,
        service_epoch: u64,
        description_fingerprint: [u8; 32],
        online_generation: u64,
        lease_generation: u64,
    },
}

#[async_trait]
pub(crate) trait EnvironmentApplyLeaseRuntime: Send + Sync + 'static {
    async fn observe_generations(
        &self,
        workspace_id: Uuid,
    ) -> AppResult<EnvironmentApplyGenerations>;

    async fn observe_resource(
        &self,
        key: &EnvironmentApplyLeaseResourceKey,
    ) -> AppResult<EnvironmentApplyLeaseResourceObservation>;

    async fn observe_android_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>>;

    fn resource_acquired(&self, _key: &EnvironmentApplyLeaseResourceKey) {}

    fn resource_released(&self, _key: &EnvironmentApplyLeaseResourceKey) {}
}

struct OwnedResourceGate {
    key: EnvironmentApplyLeaseResourceKey,
    guard: OwnedMutexGuard<()>,
}

struct ReverseReleaseGuards {
    runtime: Arc<dyn EnvironmentApplyLeaseRuntime>,
    guards: Vec<OwnedResourceGate>,
}

impl Drop for ReverseReleaseGuards {
    fn drop(&mut self) {
        while let Some(owned) = self.guards.pop() {
            let OwnedResourceGate { key, guard } = owned;
            drop(guard);
            self.runtime.resource_released(&key);
        }
    }
}

#[derive(Clone)]
pub struct EnvironmentApplyLeaseAdapter {
    runtime: Arc<dyn EnvironmentApplyLeaseRuntime>,
    pub(super) resource_gates: Arc<EnvironmentApplyResourceGateRegistry>,
}

pub(crate) struct EnvironmentApplyRuntimeAdapter {
    pub(super) listeners: Arc<dyn ListenerRuntimePort>,
    pub(super) android: Arc<dyn AndroidControlPort>,
    pub(super) external_packages: Arc<ExternalPackageRegistryAdapter>,
    sqlite: SqliteExecutor,
    pub(super) resource_gates: Arc<EnvironmentApplyResourceGateRegistry>,
}

impl EnvironmentApplyRuntimeAdapter {
    pub(crate) fn new(
        listeners: Arc<dyn ListenerRuntimePort>,
        android: Arc<dyn AndroidControlPort>,
        external_packages: Arc<ExternalPackageRegistryAdapter>,
        sqlite: SqliteExecutor,
        resource_gates: Arc<EnvironmentApplyResourceGateRegistry>,
    ) -> Self {
        Self {
            listeners,
            android,
            external_packages,
            sqlite,
            resource_gates,
        }
    }
}

impl std::fmt::Debug for EnvironmentApplyRuntimeAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentApplyRuntimeAdapter")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl EnvironmentApplyLeaseRuntime for EnvironmentApplyRuntimeAdapter {
    async fn observe_android_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        self.android.runtime_owners().await.map_err(|_| {
            stable_error(
                "APPLY_LEASE_ANDROID_UNAVAILABLE",
                "Android 运行态暂时不可读取。",
            )
        })
    }

    async fn observe_generations(
        &self,
        workspace_id: Uuid,
    ) -> AppResult<EnvironmentApplyGenerations> {
        let mut observed = self
            .sqlite
            .execute(move |store| store.observe_environment_apply_generations(workspace_id))
            .await?;
        let statuses = self.listeners.statuses().await.map_err(|_| {
            stable_error(
                "APPLY_LEASE_RUNTIME_UNAVAILABLE",
                "监听运行态暂时不可读取。",
            )
        })?;
        observed.listener =
            self.resource_gates
                .reconcile_listener_projections(statuses.iter().map(|status| {
                    (
                        status.listener_id.as_uuid(),
                        format!(
                            "{:?}:{:?}:{}",
                            status.runtime_epoch, status.state, status.active_connections
                        ),
                    )
                }));
        let owners = self.android.runtime_owners().await.map_err(|_| {
            stable_error(
                "APPLY_LEASE_ANDROID_UNAVAILABLE",
                "Android 运行态暂时不可读取。",
            )
        })?;
        observed.android =
            self.resource_gates
                .reconcile_android_projections(owners.into_iter().map(|owner| {
                    (
                        owner.profile_id,
                        owner.serial.clone(),
                        format!("{}:{:?}:{}", owner.epoch, owner.state, owner.serial),
                    )
                }));
        let external = self
            .external_packages
            .environment_apply_projections()
            .await?;
        observed.package =
            self.resource_gates
                .reconcile_exact_package_projections(external.into_iter().map(|package| {
                    let fingerprint = serde_json::to_string(&package)
                        .expect("typed package projection serialization cannot fail");
                    (package.package, fingerprint)
                }));
        Ok(observed)
    }

    async fn observe_resource(
        &self,
        key: &EnvironmentApplyLeaseResourceKey,
    ) -> AppResult<EnvironmentApplyLeaseResourceObservation> {
        match key {
            EnvironmentApplyLeaseResourceKey::Listener(listener_id) => {
                let statuses = self.listeners.statuses().await.map_err(|_| {
                    stable_error(
                        "APPLY_LEASE_RUNTIME_UNAVAILABLE",
                        "监听运行态暂时不可读取。",
                    )
                })?;
                let status = statuses
                    .iter()
                    .find(|status| status.listener_id.as_uuid() == *listener_id);
                let runtime_active = status.is_some_and(|status| {
                    status.state != ListenerRuntimeState::Stopped || status.active_connections != 0
                });
                let observation = if runtime_active {
                    EnvironmentApplyLeaseResourceObservation::ListenerActive {
                        runtime_epoch: status.and_then(|status| status.runtime_epoch),
                        active_count: status.map_or(0, |status| status.active_connections),
                    }
                } else {
                    EnvironmentApplyLeaseResourceObservation::Listener {
                        runtime_epoch: status.and_then(|status| status.runtime_epoch),
                        active_count: status.map_or(0, |status| status.active_connections),
                    }
                };
                Ok(observation)
            }
            EnvironmentApplyLeaseResourceKey::AndroidOwner { serial, .. } => {
                let owners = self.android.runtime_owners().await.map_err(|_| {
                    stable_error(
                        "APPLY_LEASE_ANDROID_UNAVAILABLE",
                        "Android 运行态暂时不可读取。",
                    )
                })?;
                let owner = owners.into_iter().find(|owner| owner.serial == *serial);
                let (owner_epoch, state) = owner.map_or_else(
                    || (None, "inactive".to_owned()),
                    |owner| {
                        let state = match owner.state {
                            AndroidRuntimeOwnerState::Active => "active",
                            AndroidRuntimeOwnerState::Uncertain => "uncertain",
                            AndroidRuntimeOwnerState::WaitingReconnect => "waiting_reconnect",
                            AndroidRuntimeOwnerState::CleanupRequired => "cleanup_required",
                            AndroidRuntimeOwnerState::StopFailed => "stop_failed",
                            AndroidRuntimeOwnerState::Faulted => "faulted",
                        }
                        .to_owned();
                        (Some(owner.epoch), state)
                    },
                );
                Ok(EnvironmentApplyLeaseResourceObservation::Android {
                    serial: serial.clone(),
                    owner_epoch,
                    state,
                })
            }
            EnvironmentApplyLeaseResourceKey::ExactPackage(package) => {
                self.observe_exact_package(package).await
            }
        }
    }
}

impl std::fmt::Debug for EnvironmentApplyLeaseAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentApplyLeaseAdapter")
            .finish_non_exhaustive()
    }
}

pub(super) fn stable_error(code: &'static str, message: &'static str) -> AppError {
    AppError::new(code, message)
}

pub(super) fn package_unavailable() -> AppError {
    stable_error(
        "APPLY_LEASE_PACKAGE_UNAVAILABLE",
        "协议包运行态暂时不可读取。",
    )
}

impl EnvironmentApplyLeaseAdapter {
    #[cfg(test)]
    pub(crate) fn new(runtime: Arc<dyn EnvironmentApplyLeaseRuntime>) -> Self {
        Self::with_resource_gates(
            runtime,
            Arc::new(EnvironmentApplyResourceGateRegistry::default()),
        )
    }

    pub(crate) fn with_resource_gates(
        runtime: Arc<dyn EnvironmentApplyLeaseRuntime>,
        resource_gates: Arc<EnvironmentApplyResourceGateRegistry>,
    ) -> Self {
        Self {
            runtime,
            resource_gates,
        }
    }

    fn canonical_scope(
        baseline: &EnvironmentValidatedApplyBaseline,
    ) -> Vec<EnvironmentApplyLeaseResourceKey> {
        let mut keys = baseline
            .affected_listeners()
            .iter()
            .map(|listener| EnvironmentApplyLeaseResourceKey::Listener(listener.listener_id()))
            .collect::<Vec<_>>();
        for owner in baseline.android_owners() {
            keys.push(EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id: owner.profile_id().to_owned(),
                serial: owner.serial().to_owned(),
            });
        }
        keys.extend(baseline.exact_packages().iter().map(|package| {
            EnvironmentApplyLeaseResourceKey::ExactPackage(package.package_ref().clone())
        }));
        keys.sort_by(super::environment_apply_resources::canonical_resource_cmp);
        keys.dedup();
        keys
    }

    fn gate(&self, key: &EnvironmentApplyLeaseResourceKey) -> Arc<AsyncMutex<()>> {
        self.resource_gates.gate(key)
    }
}

pub(crate) fn resource_matches(
    baseline: &EnvironmentValidatedApplyBaseline,
    key: &EnvironmentApplyLeaseResourceKey,
    observation: &EnvironmentApplyLeaseResourceObservation,
) -> bool {
    match (key, observation) {
        (
            EnvironmentApplyLeaseResourceKey::Listener(listener_id),
            EnvironmentApplyLeaseResourceObservation::Listener {
                runtime_epoch,
                active_count,
            },
        ) => baseline.affected_listeners().iter().any(|expected| {
            expected.listener_id() == *listener_id
                && expected.runtime_epoch() == *runtime_epoch
                && expected.active_count() == *active_count
        }),
        (
            EnvironmentApplyLeaseResourceKey::AndroidOwner { profile_id, serial },
            EnvironmentApplyLeaseResourceObservation::Android {
                serial: observed_serial,
                owner_epoch,
                state,
            },
        ) => baseline.android_owners().iter().any(|expected| {
            expected.profile_id() == profile_id
                && expected.serial() == serial
                && expected.serial() == observed_serial
                && Some(expected.owner_epoch()) == *owner_epoch
                && expected.state() == state
        }),
        (
            EnvironmentApplyLeaseResourceKey::ExactPackage(package),
            EnvironmentApplyLeaseResourceObservation::ExactPackage {
                generation,
                enabled,
                online,
                service_epoch,
                description_fingerprint,
                online_generation,
                lease_generation,
            },
        ) => baseline.exact_packages().iter().any(|expected| {
            expected.package_ref() == package
                && expected.generation() == *generation
                && expected.enabled() == *enabled
                && expected.online() == *online
                && expected.service_epoch() == *service_epoch
                && expected.description_fingerprint() == description_fingerprint
                && expected.online_generation() == *online_generation
                && expected.lease_generation() == *lease_generation
        }),
        _ => false,
    }
}

#[async_trait]
impl EnvironmentApplyLeasePort for EnvironmentApplyLeaseAdapter {
    async fn acquire(
        &self,
        request: EnvironmentApplyLeaseRequest,
    ) -> AppResult<EnvironmentApplyLease> {
        let current_owners = self.runtime.observe_android_owners().await?;
        let mut scope = Self::canonical_scope(&request.validated_baseline);
        scope.extend(current_owners.iter().map(|owner| {
            EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id: owner.profile_id.clone(),
                serial: owner.serial.clone(),
            }
        }));
        scope.sort_by(super::environment_apply_resources::canonical_resource_cmp);
        scope.dedup();
        let mut guards = ReverseReleaseGuards {
            runtime: Arc::clone(&self.runtime),
            guards: Vec::with_capacity(scope.len()),
        };
        for key in scope {
            let guard = self.gate(&key).lock_owned().await;
            self.runtime.resource_acquired(&key);
            guards.guards.push(OwnedResourceGate { key, guard });
        }

        let workspace_id = request
            .validated_baseline
            .target_workspace_id()
            .ok_or_else(|| {
                stable_error(
                    "APPLY_LEASE_BASELINE_INVALID",
                    "环境配置基线缺少目标 Workspace。",
                )
            })?;
        let observed = self
            .runtime
            .observe_generations(workspace_id)
            .await
            .map_err(|_| {
                stable_error(
                    "APPLY_LEASE_RUNTIME_UNAVAILABLE",
                    "应用运行代次暂时不可读取。",
                )
            })?;
        let android_owner_mismatch =
            !current_owners.is_empty() || !request.validated_baseline.android_owners().is_empty();
        let mut package_stale = false;
        let mut resource_mismatch = false;
        let mut runtime_active = false;
        for owned in &guards.guards {
            let observation = match self.runtime.observe_resource(&owned.key).await {
                Ok(observation) => observation,
                Err(_)
                    if matches!(owned.key, EnvironmentApplyLeaseResourceKey::ExactPackage(_)) =>
                {
                    package_stale = true;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if matches!(
                observation,
                EnvironmentApplyLeaseResourceObservation::ListenerActive { .. }
                    | EnvironmentApplyLeaseResourceObservation::Listener {
                        active_count: 1..,
                        ..
                    }
            ) {
                runtime_active = true;
                continue;
            }
            if !resource_matches(&request.validated_baseline, &owned.key, &observation) {
                if matches!(owned.key, EnvironmentApplyLeaseResourceKey::ExactPackage(_)) {
                    package_stale = true;
                } else {
                    resource_mismatch = true;
                }
            }
        }

        let release = move || drop(guards);
        package_stale |= observed.package != request.expected.package;
        Ok(if runtime_active {
            EnvironmentApplyLease::runtime_active_with_reverse_release(observed, release)
        } else if android_owner_mismatch {
            EnvironmentApplyLease::android_owner_mismatch_with_reverse_release(observed, release)
        } else if package_stale {
            EnvironmentApplyLease::package_stale_with_reverse_release(observed, release)
        } else if resource_mismatch || observed != request.expected {
            EnvironmentApplyLease::generation_mismatch_with_reverse_release(observed, release)
        } else {
            EnvironmentApplyLease::acquired_with_reverse_release(observed, release)
        })
    }
}
