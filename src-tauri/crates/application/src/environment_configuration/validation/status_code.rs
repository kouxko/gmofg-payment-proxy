use super::EnvironmentValidationLayer;
use crate::{AppError, environment_configuration::EnvironmentStatusCode};

pub(super) fn status_code_from_error(
    error: &AppError,
    layer: EnvironmentValidationLayer,
) -> EnvironmentStatusCode {
    if matches!(
        layer,
        EnvironmentValidationLayer::DnsTcpPort | EnvironmentValidationLayer::TlsMtls
    ) {
        return EnvironmentStatusCode::ValidationLayerFailed;
    }
    match error.view_model.code.as_str() {
        "SCHEMA_INVALID" => EnvironmentStatusCode::SchemaInvalid,
        "UNKNOWN_FIELD" => EnvironmentStatusCode::UnknownField,
        "FORBIDDEN_FIELD" => EnvironmentStatusCode::ForbiddenField,
        "DTO_LIMIT_EXCEEDED" => EnvironmentStatusCode::DtoLimitExceeded,
        "WORKSPACE_NAME_EMPTY" => EnvironmentStatusCode::WorkspaceNameEmpty,
        "WORKSPACE_NAME_COLLISION" => EnvironmentStatusCode::WorkspaceNameCollision,
        "LISTENER_ALIAS_DUPLICATE" => EnvironmentStatusCode::ListenerAliasDuplicate,
        "LISTENER_ALIAS_MISSING" => EnvironmentStatusCode::ListenerAliasMissing,
        "LISTENER_ALIAS_TYPE_MISMATCH" => EnvironmentStatusCode::ListenerAliasTypeMismatch,
        "LISTENER_DOMAIN_INVALID" => EnvironmentStatusCode::ListenerDomainInvalid,
        "EXISTING_RULE_ID_FORBIDDEN" => EnvironmentStatusCode::ExistingRuleIdForbidden,
        "EXISTING_RULE_ID_UNKNOWN" => EnvironmentStatusCode::ExistingRuleIdUnknown,
        "EXISTING_RULE_ID_DUPLICATE" => EnvironmentStatusCode::ExistingRuleIdDuplicate,
        "EXISTING_RULE_ID_WORKSPACE_MISMATCH" => {
            EnvironmentStatusCode::ExistingRuleIdWorkspaceMismatch
        }
        "EXISTING_RULE_ID_KIND_MISMATCH" => EnvironmentStatusCode::ExistingRuleIdKindMismatch,
        "EXISTING_RULE_ID_BINDING_MISMATCH" => EnvironmentStatusCode::ExistingRuleIdBindingMismatch,
        "EXISTING_RULE_ID_PACKAGE_MISMATCH" => EnvironmentStatusCode::ExistingRuleIdPackageMismatch,
        "EXISTING_RULE_ID_STAGE_MISMATCH" => EnvironmentStatusCode::ExistingRuleIdStageMismatch,
        "HTTP_RULE_INVALID" => EnvironmentStatusCode::HttpRuleInvalid,
        "PROTOCOL_DOCUMENT_RULE_INVALID" => EnvironmentStatusCode::UnifiedRuleInvalid,
        "WEAK_NETWORK_WIRE_INVALID" => EnvironmentStatusCode::WeakNetworkWireInvalid,
        "WEAK_NETWORK_VALUE_INVALID" => EnvironmentStatusCode::WeakNetworkValueInvalid,
        "MATERIAL_ALIAS_DUPLICATE" => EnvironmentStatusCode::MaterialAliasDuplicate,
        "MATERIAL_ALIAS_MISSING" => EnvironmentStatusCode::MaterialAliasMissing,
        "MATERIAL_ALIAS_TYPE_MISMATCH" => EnvironmentStatusCode::MaterialAliasTypeMismatch,
        "MATERIAL_ALIAS_UNUSED" => EnvironmentStatusCode::MaterialAliasUnused,
        "MATERIAL_ALIAS_MULTIPLE_CONSUMERS_UNSUPPORTED" => {
            EnvironmentStatusCode::MaterialAliasMultipleConsumersUnsupported
        }
        "UNSUPPORTED_SECRET_ROLE" => EnvironmentStatusCode::UnsupportedSecretRole,
        "UNSUPPORTED_MATERIAL_ROLE" => EnvironmentStatusCode::UnsupportedMaterialRole,
        "CERTIFICATE_PARSE_FAILED" => EnvironmentStatusCode::CertificateParseFailed,
        "CERTIFICATE_ROLE_MISMATCH" => EnvironmentStatusCode::CertificateRoleMismatch,
        "SECRET_VALUE_INVALID" => EnvironmentStatusCode::SecretValueInvalid,
        "PROTOCOL_PACKAGE_NOT_INSTALLED" => EnvironmentStatusCode::ProtocolPackageNotInstalled,
        "PROTOCOL_PACKAGE_DISABLED" => EnvironmentStatusCode::ProtocolPackageDisabled,
        "EXTERNAL_PACKAGE_OFFLINE" => EnvironmentStatusCode::ExternalPackageOffline,
        "PROTOCOL_PACKAGE_INCOMPATIBLE" => EnvironmentStatusCode::ProtocolPackageIncompatible,
        "INVALID_PROTOCOL_PACKAGE_VERSION" => EnvironmentStatusCode::InvalidProtocolPackageVersion,
        "MCP_CREATE_DEADLINE_EXCEEDED" => EnvironmentStatusCode::McpCreateDeadlineExceeded,
        _ => EnvironmentStatusCode::ValidationLayerFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_errors_preserve_their_stable_public_status_codes() {
        for (error_code, expected) in [
            (
                "CERTIFICATE_ROLE_MISMATCH",
                EnvironmentStatusCode::CertificateRoleMismatch,
            ),
            (
                "SECRET_VALUE_INVALID",
                EnvironmentStatusCode::SecretValueInvalid,
            ),
        ] {
            assert_eq!(
                status_code_from_error(
                    &AppError::new(error_code, "safe validation failure"),
                    EnvironmentValidationLayer::Material,
                ),
                expected,
            );
        }
    }

    #[test]
    fn network_layers_never_expose_adapter_error_codes() {
        for layer in [
            EnvironmentValidationLayer::DnsTcpPort,
            EnvironmentValidationLayer::TlsMtls,
        ] {
            assert_eq!(
                status_code_from_error(&AppError::new("SECRET_VALUE_INVALID", "safe"), layer),
                EnvironmentStatusCode::ValidationLayerFailed,
            );
        }
    }
}
