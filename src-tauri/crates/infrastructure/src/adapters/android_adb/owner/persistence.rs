use intercept_proxy_application::{
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerState, AndroidRuntimeOwnerViewModel,
    AppError, AppResult,
};
use uuid::Uuid;

use super::OwnerTransitionContext;
use crate::{AndroidRuntimeOwnerRecord, adapters::android_adb::AndroidOwnerState};

pub(super) async fn persist_replace(
    context: &OwnerTransitionContext,
    owner: AndroidRuntimeOwnerViewModel,
    reverse_ports: Vec<u16>,
    resume_state: Option<AndroidRuntimeOwnerState>,
    endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    active_runtime: Option<crate::adapters::android_adb::ActiveRuntimeFacts>,
) -> AppResult<bool> {
    let expected_epoch = owner.epoch;
    let record = AndroidRuntimeOwnerRecord {
        owner: owner.clone(),
        reverse_ports: reverse_ports.clone(),
        resume_state,
        runtime_endpoints: endpoints.clone(),
    };
    let replaced = context
        .executor
        .execute({
            let serial = owner.serial.clone();
            move |store| {
                store.replace_android_runtime_owner_if_epoch(&serial, expected_epoch, &record)
            }
        })
        .await
        .map_err(|error| owner_store_error(&error, &owner.serial, Some(expected_epoch)))?;
    if replaced {
        context
            .update(|state| {
                if owns_epoch(state, expected_epoch) {
                    publish_record(state, owner, reverse_ports, resume_state, endpoints);
                    if let Some(runtime) = active_runtime {
                        state.active_runtime = Some(runtime);
                    }
                }
            })
            .await;
    }
    Ok(replaced)
}

pub(super) fn owns_epoch(state: &AndroidOwnerState, epoch: Uuid) -> bool {
    state
        .runtime_owner
        .as_ref()
        .is_some_and(|owner| owner.epoch == epoch)
}

pub(super) fn reverse_ports(state: &AndroidOwnerState, expected_epoch: Uuid) -> Vec<u16> {
    state
        .active_reverse
        .as_ref()
        .filter(|reverse| reverse.epoch == expected_epoch)
        .map_or_else(Vec::new, |reverse| reverse.ports.clone())
}

pub(super) fn publish_record(
    state: &mut AndroidOwnerState,
    owner: AndroidRuntimeOwnerViewModel,
    reverse_ports: Vec<u16>,
    resume_state: Option<AndroidRuntimeOwnerState>,
    runtime_endpoints: Vec<AndroidRuntimeEndpointViewModel>,
) {
    state.active_reverse =
        (!reverse_ports.is_empty()).then(|| crate::adapters::android_adb::ActiveReverseOwnership {
            epoch: owner.epoch,
            serial: owner.serial.clone(),
            profile_id: owner.profile_id.clone(),
            ports: reverse_ports,
        });
    state.runtime_owner = Some(owner);
    state.runtime_resume_state = resume_state;
    state.runtime_endpoints = runtime_endpoints;
}

pub(super) fn owner_store_error(
    error: &crate::InfrastructureError,
    serial: &str,
    runtime_epoch: Option<Uuid>,
) -> AppError {
    let mapped = if matches!(error, crate::InfrastructureError::RevisionConflict) {
        AppError::new(
            "ANDROID_RUNTIME_ALREADY_MANAGED",
            "该 Android 设备已有登记中的网络运行态。",
        )
    } else if matches!(
        error,
        crate::InfrastructureError::Database {
            source: rusqlite::Error::SqliteFailure(_, Some(message)),
        } if message == "ANDROID_RUNTIME_CAPACITY_EXCEEDED"
    ) {
        AppError::new(
            "ANDROID_RUNTIME_CAPACITY_EXCEEDED",
            "Android 网络运行设备已达到最多 8 台的容量限制。",
        )
    } else if error
        .to_string()
        .contains("ANDROID_RUNTIME_ALREADY_MANAGED")
    {
        AppError::new(
            "ANDROID_RUNTIME_ALREADY_MANAGED",
            "该 Android 设备已有登记中的网络运行态。",
        )
    } else {
        AppError::new(
            "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED",
            format!("Android 运行设备记录读写失败：{error}"),
        )
    }
    .entity(serial);
    match runtime_epoch {
        Some(epoch) => mapped.epoch(epoch),
        None => mapped,
    }
}
