use std::collections::BTreeSet;

use intercept_proxy_application::{
    AndroidControlPort, AndroidDeviceState, AndroidDeviceTarget, AndroidDeviceViewModel,
    AndroidRuntimeOwnerState, AndroidRuntimeOwnerViewModel, AppError, AppResult,
};

use super::{AndroidAdbAdapter, COMMAND_TIMEOUT, parse_devices};

impl AndroidAdbAdapter {
    pub(super) async fn discover_devices(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        let output = self
            .run(vec!["devices".into(), "-l".into()], COMMAND_TIMEOUT)
            .await?;
        let selected = self
            .selected_serial
            .read()
            .expect("selected serial lock")
            .clone();
        let devices = parse_devices(&output.stdout, selected.as_deref());
        self.reconcile_device_owners(&devices).await?;
        Ok(devices)
    }

    pub(super) async fn reconcile_device_owners(
        &self,
        devices: &[AndroidDeviceViewModel],
    ) -> AppResult<()> {
        let online_serials = devices
            .iter()
            .filter(|device| device.state == AndroidDeviceState::Device)
            .map(|device| device.serial.as_str())
            .collect::<BTreeSet<_>>();

        let mut first_error: Option<AppError> = None;
        for owner in self.runtime_owner_snapshots().await {
            let result = self
                .reconcile_device_owner(&owner, online_serials.contains(owner.serial.as_str()))
                .await;
            if let Err(error) = result
                && first_error.is_none()
            {
                first_error = Some(
                    self.contextualize_authoritative_owner_error(&owner.serial, error)
                        .await,
                );
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn reconcile_device_owner(
        &self,
        owner: &AndroidRuntimeOwnerViewModel,
        online: bool,
    ) -> AppResult<()> {
        if online {
            if owner.state == AndroidRuntimeOwnerState::WaitingReconnect {
                AndroidControlPort::network_status(
                    self,
                    AndroidDeviceTarget {
                        serial: owner.serial.clone(),
                    },
                )
                .await?;
            }
            return Ok(());
        }
        if owner.state == AndroidRuntimeOwnerState::WaitingReconnect {
            return Ok(());
        }

        let _environment_apply_gates = self
            .acquire_environment_apply_gates(Some(&owner.profile_id), Some(&owner.serial))
            .await;
        let gate = self.device_operations.gate(&owner.serial);
        let _operation = gate.lock().await;
        let Some(current) = self.runtime_owner_snapshot_for(&owner.serial).await else {
            return Ok(());
        };
        if current.epoch != owner.epoch
            || current.state == AndroidRuntimeOwnerState::WaitingReconnect
        {
            return Ok(());
        }
        self.mark_owner_waiting_reconnect(&owner.serial, owner.epoch)
            .await
    }
}
