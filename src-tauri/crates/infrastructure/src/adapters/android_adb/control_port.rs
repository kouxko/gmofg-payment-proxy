use super::{
    AndroidAdbAdapter, COMMAND_TIMEOUT, INSTALL_TIMEOUT, combine_stop_failures,
    companion_install_view_model, consent_opened_status, control_unavailable_status,
    is_owner_unreachable, is_socket_unavailable, no_runtime_owner_status, normalize_packages,
    owner_disconnected_status, parse_package_version, parse_packages, reverse_mapping_present,
};
use async_trait::async_trait;
use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, AndroidAdbViewModel, AndroidCompanionInstallViewModel,
    AndroidControlPort, AndroidDeviceState, AndroidDeviceTarget, AndroidDeviceViewModel,
    AndroidNetworkActivation, AndroidNetworkStatusViewModel, AndroidPackageViewModel,
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerViewModel, AndroidRuntimeTarget, AppError, AppResult,
};
use serde_json::json;

#[async_trait]
impl AndroidControlPort for AndroidAdbAdapter {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        self.adb_view().await
    }

    async fn adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel> {
        let devices = self.device_list().await?;
        let device = devices
            .iter()
            .find(|device| device.serial == serial)
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_DEVICE_NOT_FOUND",
                    "所选 Android 设备不在 adb 列表中。",
                )
            })?;
        if device.state != AndroidDeviceState::Device {
            return Err(AppError::new(
                "ANDROID_DEVICE_NOT_READY",
                "所选 Android 设备未在线或未授权。",
            ));
        }
        *self.selected_serial.write().expect("selected serial lock") = Some(serial);
        self.adb_get().await
    }

    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        self.discover_devices().await
    }

    async fn package_list(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        let serial = target.serial;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let output = self
            .run_for_serial(
                &serial,
                &["shell", "pm", "list", "packages", "-U"],
                COMMAND_TIMEOUT,
            )
            .await?;
        Ok(normalize_packages(parse_packages(&output.stdout)))
    }

    async fn package_get(
        &self,
        target: AndroidDeviceTarget,
        package_name: String,
    ) -> AppResult<AndroidPackageViewModel> {
        self.package_list(target)
            .await?
            .into_iter()
            .find(|package| package.package_name == package_name)
            .ok_or_else(|| {
                AppError::new("ANDROID_PACKAGE_NOT_FOUND", "设备上未找到指定 Android 包。")
            })
    }

    async fn companion_install(
        &self,
        target: AndroidDeviceTarget,
        update: bool,
    ) -> AppResult<AndroidCompanionInstallViewModel> {
        let serial = target.serial;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let apk = self
            .companion_apk
            .as_ref()
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_COMPANION_APK_NOT_FOUND",
                    "桌面资源中未找到 android-companion APK。",
                )
            })?;
        let mut args = vec!["-s".into(), serial.clone(), "install".into()];
        if update {
            args.push("-r".into());
        }
        args.push(apk.display().to_string());
        let output = self.run(args, INSTALL_TIMEOUT).await?;
        if !output.stdout.lines().any(|line| line.trim() == "Success") {
            return Err(AppError::new(
                "ANDROID_COMPANION_INSTALL_UNVERIFIED",
                "adb install 未返回 Success。",
            ));
        }
        let dump = self
            .run_for_serial(
                &serial,
                &["shell", "dumpsys", "package", ANDROID_COMPANION_PACKAGE],
                COMMAND_TIMEOUT,
            )
            .await?;
        let (version_name, version_code) = parse_package_version(&dump.stdout);
        Ok(companion_install_view_model(
            serial,
            version_name,
            version_code,
        ))
    }

    async fn vpn_open_consent(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = target.serial;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        self.run_for_serial(
            &serial,
            &[
                "shell",
                "am",
                "start",
                "-W",
                "-n",
                "com.interceptproxy.vpn/.VpnConsentActivity",
            ],
            COMMAND_TIMEOUT,
        )
        .await?;
        Ok(consent_opened_status(serial))
    }

    async fn network_start(
        &self,
        target: AndroidDeviceTarget,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = target.serial;
        let _environment_apply_gates = self
            .acquire_environment_apply_gates(Some(activation.profile.id.as_str()), Some(&serial))
            .await;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let prepared = self
            .prepare_usb_proxy_runtime(&serial, None, &activation, AndroidRuntimeOwnerSource::Start)
            .await?;
        let runtime_epoch = prepared.owner.epoch;
        let payload = super::lease::control_request_payload(
            &activation.profile,
            &prepared.payload,
            runtime_epoch,
        );
        let accepted = match self
            .protocol_request(&serial, "start", payload.clone())
            .await
        {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake(&serial, "start", payload)
                    .await
            }
            Err(error) => Err(error),
        };
        let result = match accepted {
            Ok(status) => match self
                .confirm_network_running(&prepared.runtime, status)
                .await
            {
                Ok(confirmed) => {
                    self.finish_prepared_network_update(prepared, Ok(confirmed))
                        .await
                }
                Err(error) if error.view_model.code == "ANDROID_NETWORK_START_FAILED" => {
                    self.finish_prepared_network_update(prepared, Err(error))
                        .await
                }
                Err(error) => self.retain_uncertain_network_update(prepared, error).await,
            },
            Err(error) => {
                self.finish_prepared_network_update(prepared, Err(error))
                    .await
            }
        };
        result.map(|mut status| {
            status.runtime_epoch = Some(runtime_epoch);
            status
        })
    }

    async fn network_apply(
        &self,
        target: AndroidRuntimeTarget,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = target.serial;
        let _environment_apply_gates = self
            .acquire_environment_apply_gates(Some(activation.profile.id.as_str()), Some(&serial))
            .await;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        self.required_runtime_target(&serial, target.expected_epoch)
            .await?;
        let prepared = self
            .prepare_usb_proxy_runtime(
                &serial,
                Some(target.expected_epoch),
                &activation,
                AndroidRuntimeOwnerSource::Apply,
            )
            .await?;
        let runtime_epoch = prepared.owner.epoch;
        let payload = super::lease::control_request_payload(
            &activation.profile,
            &prepared.payload,
            runtime_epoch,
        );
        let accepted = match self
            .protocol_request(&serial, "apply", payload.clone())
            .await
        {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake(&serial, "apply", payload)
                    .await
            }
            Err(error) => Err(error),
        };
        let result = match accepted {
            Ok(status) => match self
                .confirm_network_running(&prepared.runtime, status)
                .await
            {
                Ok(confirmed) => {
                    self.finish_prepared_network_update(prepared, Ok(confirmed))
                        .await
                }
                Err(error) if error.view_model.code == "ANDROID_NETWORK_START_FAILED" => {
                    self.finish_prepared_network_update(prepared, Err(error))
                        .await
                }
                Err(error) => self.retain_uncertain_network_update(prepared, error).await,
            },
            Err(error) => {
                self.finish_prepared_network_update(prepared, Err(error))
                    .await
            }
        };
        result.map(|mut status| {
            status.runtime_epoch = Some(runtime_epoch);
            status
        })
    }

    async fn network_runtime_ready(
        &self,
        target: AndroidDeviceTarget,
        activation: &AndroidNetworkActivation,
        status: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        let serial = target.serial;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let Some(owner) = self.runtime_owner_snapshot_for(&serial).await else {
            return Ok(false);
        };
        let active_runtime = self.owner_state_snapshot_for(&serial).await.active_runtime;
        let Some(active_runtime) = active_runtime.filter(|runtime| {
            runtime.serial == serial && runtime.profile_id == activation.profile.id
        }) else {
            return Ok(false);
        };
        if status.active_profile_fingerprint.as_deref()
            != Some(active_runtime.profile_fingerprint.as_str())
            || status.active_route_fingerprint.as_deref()
                != Some(active_runtime.route_fingerprint.as_str())
            || status.active_route_count != active_runtime.route_count
        {
            return Ok(false);
        }
        if activation.proxy_routes.is_empty() {
            return Ok(true);
        }
        if !active_runtime.uses_adb_reverse {
            return Ok(owner.state == AndroidRuntimeOwnerState::Active);
        }
        let listing = self
            .run_for_serial(&serial, &["reverse", "--list"], COMMAND_TIMEOUT)
            .await?
            .stdout;
        let listener_ports = active_runtime.listener_ports;
        Ok(listener_ports.iter().all(|(listener_id, device_port)| {
            activation
                .proxy_routes
                .iter()
                .find(|route| route.listener_id == *listener_id)
                .is_some_and(|route| {
                    reverse_mapping_present(&listing, *device_port, route.desktop_listener_port)
                })
        }))
    }

    async fn network_stop(
        &self,
        target: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = target.serial;
        let owner_for_gate = self.runtime_owner_snapshot_for(&serial).await;
        let _environment_apply_gates = self
            .acquire_environment_apply_gates(
                owner_for_gate
                    .as_ref()
                    .map(|owner| owner.profile_id.as_str()),
                owner_for_gate.as_ref().map(|owner| owner.serial.as_str()),
            )
            .await;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let owner = self
            .required_runtime_target(&serial, target.expected_epoch)
            .await?;
        let graceful = match self
            .protocol_request(&owner.serial, "stop", json!({}))
            .await
        {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake(&owner.serial, "stop", json!({}))
                    .await
            }
            Err(error) => Err(error),
        };
        let result = match graceful {
            Ok(status) => Ok(status),
            Err(graceful_error) => match self.force_stop_companion(&owner.serial).await {
                Ok(status) => Ok(status),
                Err(force_error) => Err(combine_stop_failures(graceful_error, &force_error)),
            },
        };
        let combined = match result {
            Ok(status) => self.cleanup_owner_reverse(&owner).await.map(|()| status),
            Err(error) => Err(error),
        };
        match combined {
            Ok(mut status) => {
                status.runtime_epoch = Some(owner.epoch);
                if !self
                    .clear_owner_if_epoch_under_gate(&serial, owner.epoch)
                    .await?
                {
                    return Err(self.runtime_owner_conflict_error(&serial).await);
                }
                Ok(status)
            }
            Err(error) => {
                self.mark_owner_stop_failed(&serial, owner.epoch, error.view_model.message.clone())
                    .await?;
                Err(error)
            }
        }
    }

    async fn emergency_restore(
        &self,
        target: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = target.serial;
        let owner_for_gate = self.runtime_owner_snapshot_for(&serial).await;
        let _environment_apply_gates = self
            .acquire_environment_apply_gates(
                owner_for_gate
                    .as_ref()
                    .map(|owner| owner.profile_id.as_str()),
                owner_for_gate.as_ref().map(|owner| owner.serial.as_str()),
            )
            .await;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let owner = self
            .required_runtime_target(&serial, target.expected_epoch)
            .await?;
        let force_stop = self.force_stop_companion(&owner.serial).await;
        let combined = match force_stop {
            Ok(status) => self.cleanup_owner_reverse(&owner).await.map(|()| status),
            Err(error) => Err(error),
        };
        match combined {
            Ok(mut status) => {
                status.runtime_epoch = Some(owner.epoch);
                if !self
                    .clear_owner_if_epoch_under_gate(&serial, owner.epoch)
                    .await?
                {
                    return Err(self.runtime_owner_conflict_error(&serial).await);
                }
                Ok(status)
            }
            Err(error) => {
                self.mark_owner_stop_failed(&serial, owner.epoch, error.view_model.message.clone())
                    .await?;
                Err(error)
            }
        }
    }

    async fn network_status(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = target.serial;
        let owner_for_gate = self.runtime_owner_snapshot_for(&serial).await;
        let _environment_apply_gates = self
            .acquire_environment_apply_gates(
                owner_for_gate
                    .as_ref()
                    .map(|owner| owner.profile_id.as_str()),
                Some(&serial),
            )
            .await;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let Some(owner) = self.runtime_owner_snapshot_for(&serial).await else {
            return Ok(no_runtime_owner_status(serial));
        };
        match self
            .protocol_request(&owner.serial, "status", json!({}))
            .await
        {
            Ok(mut status) => {
                status.runtime_epoch = Some(owner.epoch);
                self.mark_owner_reconnected(&serial, owner.epoch, Some(status.state))
                    .await?;
                Ok(status)
            }
            Err(error) if is_socket_unavailable(&error) => {
                let serial = owner.serial;
                let output = self
                    .run_for_serial(
                        &serial,
                        &["shell", "pidof", ANDROID_COMPANION_PACKAGE],
                        COMMAND_TIMEOUT,
                    )
                    .await;
                match output {
                    Ok(output) => {
                        self.mark_owner_reconnected(&serial, owner.epoch, None)
                            .await?;
                        let mut status =
                            control_unavailable_status(serial, !output.stdout.trim().is_empty());
                        status.runtime_epoch = Some(owner.epoch);
                        Ok(status)
                    }
                    Err(error) if is_owner_unreachable(&error) => {
                        self.mark_owner_waiting_reconnect(&serial, owner.epoch)
                            .await?;
                        let mut status = owner_disconnected_status(serial);
                        status.runtime_epoch = Some(owner.epoch);
                        Ok(status)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) if is_owner_unreachable(&error) => {
                self.mark_owner_waiting_reconnect(&serial, owner.epoch)
                    .await?;
                let mut status = owner_disconnected_status(owner.serial);
                status.runtime_epoch = Some(owner.epoch);
                Ok(status)
            }
            Err(error) => Err(error),
        }
    }

    async fn runtime_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        self.authoritative_runtime_owners().await
    }

    async fn network_runtime_endpoints(
        &self,
        target: AndroidDeviceTarget,
        activation: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        self.reconcile_runtime_endpoints(target.serial, activation)
            .await
    }
}
