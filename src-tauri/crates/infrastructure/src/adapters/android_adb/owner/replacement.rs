use intercept_proxy_application::{
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerState, AndroidRuntimeOwnerViewModel,
    AppResult,
};

use super::{
    AndroidAdbAdapter, AndroidOwnerState, AndroidRuntimeOwnerRecord, PreparedUsbProxyRuntime,
    owner_store_error, owns_epoch, persist_replace, publish_record, reverse_ports,
};

impl AndroidAdbAdapter {
    pub(in crate::adapters::android_adb) async fn replace_owner_if_epoch(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        reverse_ports: Vec<u16>,
    ) -> AppResult<bool> {
        self.replace_owner_with_resume(owner, reverse_ports, None, false)
            .await
    }

    pub(in crate::adapters::android_adb) async fn replace_owner_endpoints_if_epoch(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    ) -> AppResult<bool> {
        let serial = owner.serial.clone();
        self.run_owner_transition(&serial, move |context| async move {
            let state = context.snapshot().await;
            if !owns_epoch(&state, owner.epoch) {
                return Ok(false);
            }
            let ports = reverse_ports(&state, owner.epoch);
            let resume_state = state.runtime_resume_state;
            persist_replace(&context, owner, ports, resume_state, endpoints, None).await
        })
        .await
    }

    pub(in crate::adapters::android_adb) async fn replace_owner_endpoints_and_runtime_if_epoch(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        endpoints: Vec<AndroidRuntimeEndpointViewModel>,
        runtime: super::super::ActiveRuntimeFacts,
    ) -> AppResult<bool> {
        let serial = owner.serial.clone();
        self.run_owner_transition(&serial, move |context| async move {
            let state = context.snapshot().await;
            if !owns_epoch(&state, owner.epoch) {
                return Ok(false);
            }
            let ports = reverse_ports(&state, owner.epoch);
            let resume_state = state.runtime_resume_state;
            persist_replace(
                &context,
                owner,
                ports,
                resume_state,
                endpoints,
                Some(runtime),
            )
            .await
        })
        .await
    }

    async fn replace_owner_with_resume(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        reverse_ports: Vec<u16>,
        explicit_resume_state: Option<AndroidRuntimeOwnerState>,
        replace_resume_state: bool,
    ) -> AppResult<bool> {
        let serial = owner.serial.clone();
        self.run_owner_transition(&serial, move |context| async move {
            let state = context.snapshot().await;
            if !owns_epoch(&state, owner.epoch) {
                return Ok(false);
            }
            let resume_state = if replace_resume_state {
                explicit_resume_state
            } else {
                state.runtime_resume_state
            };
            let endpoints = state.runtime_endpoints;
            persist_replace(
                &context,
                owner,
                reverse_ports,
                resume_state,
                endpoints,
                None,
            )
            .await
        })
        .await
    }

    pub(in crate::adapters::android_adb) async fn restore_previous_owner(
        &self,
        prepared: &PreparedUsbProxyRuntime,
    ) -> AppResult<()> {
        let prepared = prepared.clone();
        let serial = prepared.owner.serial.clone();
        let transition_serial = serial.clone();
        self.run_owner_transition(&transition_serial, move |context| async move {
            if let Some(owner) = prepared.previous_owner {
                let ports = prepared
                    .previous_reverse
                    .as_ref()
                    .map_or_else(Vec::new, |reverse| reverse.ports.clone());
                let record = AndroidRuntimeOwnerRecord {
                    owner: owner.clone(),
                    reverse_ports: ports.clone(),
                    resume_state: prepared.previous_resume_state,
                    runtime_endpoints: prepared.previous_endpoints.clone(),
                };
                let serial_for_store = owner.serial.clone();
                let error_serial = owner.serial.clone();
                let epoch = prepared.owner.epoch;
                let replaced = context
                    .executor
                    .execute(move |store| {
                        store.replace_android_runtime_owner_if_epoch(
                            &serial_for_store,
                            epoch,
                            &record,
                        )
                    })
                    .await
                    .map_err(|error| owner_store_error(&error, &error_serial, Some(epoch)))?;
                if !replaced {
                    return Err(context.runtime_owner_conflict_error().await);
                }
                context
                    .update(|state| {
                        publish_record(
                            state,
                            owner,
                            ports,
                            prepared.previous_resume_state,
                            prepared.previous_endpoints,
                        );
                        state.active_runtime = prepared.previous_runtime;
                    })
                    .await;
            } else {
                let expected_epoch = prepared.owner.epoch;
                let serial_for_store = serial.clone();
                let error_serial = serial.clone();
                let cleared = context
                    .executor
                    .execute(move |store| {
                        store.clear_android_runtime_owner(&serial_for_store, expected_epoch)
                    })
                    .await
                    .map_err(|error| {
                        owner_store_error(&error, &error_serial, Some(expected_epoch))
                    })?;
                if cleared {
                    context
                        .update(|state| {
                            if owns_epoch(state, expected_epoch) {
                                *state = AndroidOwnerState::default();
                            }
                        })
                        .await;
                } else {
                    return Err(context.runtime_owner_conflict_error().await);
                }
            }
            Ok(())
        })
        .await
    }
}
