use std::collections::BTreeMap;

use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

use super::{
    EnvironmentDnsTcpTarget, EnvironmentTlsMtlsTarget, EnvironmentValidationLayer,
    EnvironmentValidationLayerRequest,
};
use crate::environment_configuration::EnvironmentConfigurationParseError;
use crate::{AppError, AppResult};

pub(super) struct ValidationProjection {
    exact_package_refs: Vec<ProtocolPackageRef>,
    dns_tcp_targets: Vec<EnvironmentDnsTcpTarget>,
    tls_mtls_targets: Vec<EnvironmentTlsMtlsTarget>,
    installation_root_selectors: Vec<String>,
    candidate: crate::environment_configuration::EnvironmentConfigurationCandidateV1,
}

impl ValidationProjection {
    pub(super) fn parse_schema(
        candidate_json: &[u8],
    ) -> AppResult<crate::environment_configuration::EnvironmentConfigurationCandidateV1> {
        if candidate_json.len() > 1_048_576 {
            return Err(AppError::new(
                "DTO_LIMIT_EXCEEDED",
                "environment candidate exceeds its DTO limit",
            ));
        }
        crate::parse_environment_configuration_candidate_v1(candidate_json)
            .map_err(|error| parse_schema_error(&error))
    }

    pub(super) fn project(
        candidate: crate::environment_configuration::EnvironmentConfigurationCandidateV1,
    ) -> AppResult<Self> {
        Self::project_with_checkpoint(candidate, &super::NoopEnvironmentValidationCheckpoint)
    }

    pub(super) fn project_with_checkpoint(
        candidate: crate::environment_configuration::EnvironmentConfigurationCandidateV1,
        checkpoint: &dyn super::EnvironmentValidationCheckpoint,
    ) -> AppResult<Self> {
        validate_domain_graph(&candidate, checkpoint)?;
        let mut packages = BTreeMap::new();
        for package in candidate
            .workspace
            .listeners
            .iter()
            .flat_map(|listener| listener.package_refs())
            .chain(candidate.workspace.protocol_rules.iter().map(
                crate::environment_configuration::rules::ProtocolDocumentRuleTemplate::package_ref,
            ))
        {
            ensure_running(checkpoint)?;
            let package_ref = ProtocolPackageRef {
                id: ProtocolPackageId::new(package.id.clone()).map_err(AppError::from)?,
                version: ProtocolPackageVersion::new(package.version.clone()).map_err(|_| {
                    AppError::new(
                        "INVALID_PROTOCOL_PACKAGE_VERSION",
                        "invalid package version",
                    )
                })?,
            };
            packages.insert((package.id.clone(), package.version.clone()), package_ref);
        }
        let mut dns = BTreeMap::new();
        let mut tls = BTreeMap::new();
        for listener in &candidate.workspace.listeners {
            ensure_running(checkpoint)?;
            let Some(target) = listener.network_target()? else {
                continue;
            };
            dns.insert(
                (target.host.clone(), target.port),
                EnvironmentDnsTcpTarget {
                    host: target.host.clone(),
                    port: target.port,
                },
            );
            if target.uses_tls {
                tls.insert(
                    (
                        target.host.clone(),
                        target.port,
                        target.server_name.clone(),
                        target.upstream_ca_alias.clone(),
                        target.client_identity_alias.clone(),
                        target.verify_hostname,
                    ),
                    EnvironmentTlsMtlsTarget {
                        host: target.host,
                        port: target.port,
                        server_name: target.server_name,
                        upstream_ca_alias: target.upstream_ca_alias,
                        client_identity_alias: target.client_identity_alias,
                        verify_hostname: target.verify_hostname,
                    },
                );
            }
        }
        if dns.len() > 16 {
            return Err(limit_error());
        }
        let installation_root_selectors = candidate
            .workspace
            .listeners
            .iter()
            .filter_map(|listener| listener.installation_root_selector().map(str::to_owned))
            .collect();
        Ok(Self {
            exact_package_refs: packages.into_values().collect(),
            dns_tcp_targets: dns.into_values().collect(),
            tls_mtls_targets: tls.into_values().collect(),
            installation_root_selectors,
            candidate,
        })
    }

    pub(super) fn request(
        &self,
        layer: EnvironmentValidationLayer,
    ) -> EnvironmentValidationLayerRequest<'_> {
        EnvironmentValidationLayerRequest {
            layer,
            exact_package_refs: if layer == EnvironmentValidationLayer::PackageProjection {
                &self.exact_package_refs
            } else {
                &[]
            },
            dns_tcp_targets: if layer == EnvironmentValidationLayer::DnsTcpPort {
                &self.dns_tcp_targets
            } else {
                &[]
            },
            tls_mtls_targets: if layer == EnvironmentValidationLayer::TlsMtls {
                &self.tls_mtls_targets
            } else {
                &[]
            },
            installation_root_selectors: if layer == EnvironmentValidationLayer::TlsMtls {
                &self.installation_root_selectors
            } else {
                &[]
            },
            materials: matches!(
                layer,
                EnvironmentValidationLayer::Material | EnvironmentValidationLayer::TlsMtls
            )
            .then_some(&self.candidate.materials),
        }
    }

    pub(super) const fn candidate(
        &self,
    ) -> &crate::environment_configuration::EnvironmentConfigurationCandidateV1 {
        &self.candidate
    }
}

fn parse_schema_error(error: &EnvironmentConfigurationParseError) -> AppError {
    let code = match error {
        EnvironmentConfigurationParseError::PersistedIdentityForNewTarget => {
            "EXISTING_RULE_ID_FORBIDDEN"
        }
        EnvironmentConfigurationParseError::DuplicateHttpRuleSelector
        | EnvironmentConfigurationParseError::DuplicateProtocolRuleSelector => {
            "EXISTING_RULE_ID_DUPLICATE"
        }
        EnvironmentConfigurationParseError::WeakNetworkValueInvalid => "WEAK_NETWORK_VALUE_INVALID",
        EnvironmentConfigurationParseError::UnknownField => "UNKNOWN_FIELD",
        EnvironmentConfigurationParseError::ForbiddenField => "FORBIDDEN_FIELD",
        EnvironmentConfigurationParseError::DocumentValueWireInvalid => {
            "DOCUMENT_VALUE_WIRE_INVALID"
        }
        EnvironmentConfigurationParseError::WeakNetworkWireInvalid => "WEAK_NETWORK_WIRE_INVALID",
        EnvironmentConfigurationParseError::UnsupportedMaterialRole => "UNSUPPORTED_MATERIAL_ROLE",
        EnvironmentConfigurationParseError::UnsupportedSecretRole => "UNSUPPORTED_SECRET_ROLE",
        EnvironmentConfigurationParseError::InvalidJson(_)
        | EnvironmentConfigurationParseError::UnsupportedSchemaVersion => "SCHEMA_INVALID",
    };
    AppError::new(code, "environment schema validation failed")
}

#[expect(
    clippy::too_many_lines,
    reason = "the ordered validation graph preserves stable first-failure error precedence"
)]
fn validate_domain_graph(
    candidate: &crate::environment_configuration::EnvironmentConfigurationCandidateV1,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<()> {
    ensure_running(checkpoint)?;
    if candidate.workspace.listeners.len() > 8
        || candidate.workspace.http_rules.len() > 128
        || candidate.workspace.protocol_rules.len() > 128
        || candidate.materials.certificates.len() > 16
        || candidate.materials.secrets.len() > 16
        || candidate.workspace.android_network_profiles.len() > 8
    {
        return Err(limit_error());
    }
    candidate.materials.validate_domain_limits()?;
    for profile in &candidate.workspace.android_network_profiles {
        ensure_running(checkpoint)?;
        profile.validate_domain_limits()?;
    }
    if matches!(
        &candidate.target,
        crate::environment_configuration::EnvironmentTarget::New { name } if name.trim().is_empty()
    ) {
        return Err(AppError::new(
            "WORKSPACE_NAME_EMPTY",
            "workspace domain validation failed",
        ));
    }
    let mut listener_aliases = std::collections::BTreeSet::new();
    let mut enabled_endpoints = std::collections::BTreeSet::new();
    for listener in &candidate.workspace.listeners {
        ensure_running(checkpoint)?;
        if !listener_aliases.insert(listener.alias()) {
            return Err(AppError::new(
                "LISTENER_ALIAS_DUPLICATE",
                "listener alias graph validation failed",
            ));
        }
        listener.validate_domain()?;
        if listener
            .enabled_endpoint()
            .is_some_and(|endpoint| !enabled_endpoints.insert(endpoint))
        {
            return Err(AppError::new(
                "LISTENER_DOMAIN_INVALID",
                "listener endpoint graph validation failed",
            ));
        }
    }
    for alias in candidate
        .workspace
        .http_rules
        .iter()
        .map(crate::environment_configuration::rules::HttpRuleTemplate::listener_alias)
        .chain(candidate.workspace.protocol_rules.iter().map(
            crate::environment_configuration::rules::ProtocolDocumentRuleTemplate::listener_alias,
        ))
        .chain(
            candidate
                .workspace
                .android_network_profiles
                .iter()
                .flat_map(crate::environment_configuration::android::AndroidNetworkProfileTemplate::listener_aliases),
        )
    {
        ensure_running(checkpoint)?;
        if !listener_aliases.contains(alias) {
            return Err(AppError::new(
                "LISTENER_ALIAS_MISSING",
                "listener alias graph validation failed",
            ));
        }
    }
    for rule in &candidate.workspace.http_rules {
        ensure_running(checkpoint)?;
        if rule.existing_rule_id.is_none() {
            rule.validate_domain()?;
        }
    }

    let certificates = collect_roles(candidate.materials.certificate_roles(), checkpoint)?;
    let secrets = collect_roles(candidate.materials.secret_roles(), checkpoint)?;
    if certificates.keys().any(|alias| secrets.contains_key(alias)) {
        return Err(AppError::new(
            "MATERIAL_ALIAS_DUPLICATE",
            "material alias graph validation failed",
        ));
    }
    let mut consumers = BTreeMap::new();
    for (alias, expected_role, secret) in candidate.workspace.listeners.iter().flat_map(
        crate::environment_configuration::listener::ListenerTemplate::referenced_materials,
    ) {
        ensure_running(checkpoint)?;
        let roles = if secret { &secrets } else { &certificates };
        match roles.get(alias) {
            None => {
                return Err(AppError::new(
                    "MATERIAL_ALIAS_MISSING",
                    "material alias graph validation failed",
                ));
            }
            Some(actual) if *actual != expected_role => {
                return Err(AppError::new(
                    "MATERIAL_ALIAS_TYPE_MISMATCH",
                    "material alias graph validation failed",
                ));
            }
            Some(_) => {
                *consumers.entry(alias).or_insert(0_usize) += 1;
            }
        }
    }
    for (alias, role) in certificates.iter().chain(secrets.iter()) {
        ensure_running(checkpoint)?;
        match consumers.get(alias).copied().unwrap_or(0) {
            0 => {
                return Err(AppError::new(
                    "MATERIAL_ALIAS_UNUSED",
                    "material alias graph validation failed",
                ));
            }
            count
                if count > 1
                    && !matches!(*role, "downstream_client_trust" | "upstream_server_trust") =>
            {
                return Err(AppError::new(
                    "MATERIAL_ALIAS_MULTIPLE_CONSUMERS_UNSUPPORTED",
                    "material alias graph validation failed",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn ensure_running(checkpoint: &dyn super::EnvironmentValidationCheckpoint) -> AppResult<()> {
    if checkpoint.checkpoint() {
        Err(AppError::new(
            "VALIDATION_INTERRUPTED",
            "environment validation interrupted",
        ))
    } else {
        Ok(())
    }
}

fn limit_error() -> AppError {
    AppError::new(
        "DTO_LIMIT_EXCEEDED",
        "environment candidate exceeds its DTO limit",
    )
}

fn collect_roles<'a>(
    roles: impl Iterator<Item = (&'a str, &'a str)>,
    checkpoint: &dyn super::EnvironmentValidationCheckpoint,
) -> AppResult<BTreeMap<&'a str, &'a str>> {
    let mut collected = BTreeMap::new();
    for (alias, role) in roles {
        ensure_running(checkpoint)?;
        if alias.trim().is_empty() || collected.insert(alias, role).is_some() {
            return Err(AppError::new(
                "MATERIAL_ALIAS_DUPLICATE",
                "material alias graph validation failed",
            ));
        }
    }
    Ok(collected)
}
