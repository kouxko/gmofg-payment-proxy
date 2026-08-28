use std::collections::{BTreeMap, BTreeSet};

use intercept_proxy_domain::{
    AndroidDestinationTarget, AndroidNetworkProfile, AndroidProxyRoute, AndroidTargetApplication,
    ListenerId, WeakNetworkProfile,
};

use super::{AndroidNetworkProfileTemplate, AppError, AppResult};

impl AndroidNetworkProfileTemplate {
    pub(crate) fn to_domain(
        &self,
        id: String,
        listeners: &BTreeMap<&str, ListenerId>,
    ) -> AppResult<AndroidNetworkProfile> {
        Ok(AndroidNetworkProfile {
            id,
            name: self.name.trim().to_owned(),
            target_applications: self
                .target_applications
                .iter()
                .map(|target| AndroidTargetApplication {
                    package_name: target.package_name.clone(),
                    uid: target.uid,
                    display_name: target.display_name.clone(),
                })
                .collect(),
            destination_targets: self
                .destination_targets
                .iter()
                .map(|target| AndroidDestinationTarget {
                    cidr: target.cidr.clone(),
                    ports: target.ports.clone(),
                })
                .collect(),
            proxy_routes: self
                .proxy_routes
                .iter()
                .map(|route| {
                    Ok(AndroidProxyRoute {
                        destination: route.destination.clone(),
                        ports: route.ports.clone(),
                        listener_id: *listeners.get(route.listener_alias.as_str()).ok_or_else(
                            || {
                                AppError::new(
                                    "LISTENER_ALIAS_MISSING",
                                    "listener alias graph validation failed",
                                )
                            },
                        )?,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?,
            confirmed_shared_uids: self
                .confirmed_shared_uids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            auto_resume_after_reboot: self.auto_resume_after_reboot,
            stop_vpn_on_control_loss: self.stop_vpn_on_control_loss,
            weak_network: serde_json::from_value::<WeakNetworkProfile>(
                serde_json::to_value(&self.weak_network).map_err(|_| weak_network_error())?,
            )
            .map_err(|_| weak_network_error())?,
        })
    }
}

fn weak_network_error() -> AppError {
    AppError::new(
        "WEAK_NETWORK_WIRE_INVALID",
        "weak network projection failed",
    )
}
