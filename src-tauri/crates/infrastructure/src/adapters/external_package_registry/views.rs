//! 外部协议包领域注册合同到应用查询模型的纯映射。

use chrono::Utc;
use intercept_proxy_application::{
    ExternalPackageDetailViewModel, ExternalPackageDirectionMethodsViewModel,
    ExternalPackageRecentErrorViewModel, ProtocolPackageCapabilitiesViewModel,
    ProtocolPackageDescriptionViewModel, ProtocolPackageDirectionCapabilitiesViewModel,
    ProtocolPackageKindViewModel, ProtocolPackageSchemaViewModel, ProtocolPackageSourceViewModel,
    ProtocolPackageValidationViewModel, ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::DocumentSchemaNode;
use intercept_proxy_package_contract::{PackageKind, PackageManifest};

use super::ConnectionDetailSnapshot;
use crate::adapters::PackageTransportError;
use crate::sqlite::external_packages::StoredExternalPackage;

pub(super) fn application_summary(
    stored: &StoredExternalPackage,
    online: bool,
) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        package: stored.registration.package().identity().clone(),
        name: stored.registration.package().name().to_owned(),
        host_api: stored.registration.api(),
        kind: application_kind(stored.registration.kind()),
        source: ProtocolPackageSourceViewModel::External { online },
        enabled: stored.enabled,
        validation: ProtocolPackageValidationViewModel::Valid,
        installed_at: stored.first_connected_at,
    }
}

pub(crate) fn application_description(
    registration: &PackageManifest,
) -> ProtocolPackageDescriptionViewModel {
    let capabilities = ProtocolPackageDirectionCapabilitiesViewModel {
        frame: true,
        decode: true,
        encode: true,
    };
    ProtocolPackageDescriptionViewModel {
        package: registration.package().identity().clone(),
        kind: application_kind(registration.kind()),
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: capabilities,
            downstream: capabilities,
            display: true,
        },
        upstream_schema: registration
            .document()
            .upstream()
            .schema()
            .map(application_schema),
        downstream_schema: registration
            .document()
            .downstream()
            .schema()
            .map(application_schema),
    }
}

pub(super) fn application_detail(
    stored: &StoredExternalPackage,
    connection: Option<&ConnectionDetailSnapshot>,
    online: bool,
) -> ExternalPackageDetailViewModel {
    ExternalPackageDetailViewModel {
        local_process: stored.local_archive.is_some(),
        remote_address: connection
            .and_then(|detail| detail.remote_address)
            .or(stored.remote_address)
            .map(|address| address.to_string()),
        connection_id: online
            .then(|| connection.map(|detail| detail.connection_id.as_uuid()))
            .flatten(),
        first_connected_at: stored.first_connected_at,
        last_connected_at: stored.last_connected_at,
        registration_fingerprint_sha256: sha256_hex(&stored.fingerprint),
        upstream_methods: direction_methods(true),
        downstream_methods: direction_methods(false),
        recent_error: connection
            .and_then(|detail| detail.recent_error.clone())
            .or_else(|| {
                stored.recent_error.as_ref().map(|error| {
                    intercept_proxy_application::ExternalPackageRecentErrorViewModel {
                        code: error.code.clone(),
                        message: error.message.clone(),
                        occurred_at: error.occurred_at,
                    }
                })
            }),
    }
}

fn sha256_hex(fingerprint: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(fingerprint.len() * 2);
    for byte in fingerprint {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn direction_methods(upstream: bool) -> ExternalPackageDirectionMethodsViewModel {
    let direction = if upstream { "upstream" } else { "downstream" };
    ExternalPackageDirectionMethodsViewModel {
        frame: format!("hooks.{direction}.frame"),
        decode: format!("hooks.{direction}.decode"),
        encode: format!("hooks.{direction}.encode"),
        display: format!("document.{direction}.display"),
    }
}

const fn application_kind(kind: PackageKind) -> ProtocolPackageKindViewModel {
    match kind {
        PackageKind::Http => ProtocolPackageKindViewModel::Http,
        PackageKind::Socket => ProtocolPackageKindViewModel::Socket,
    }
}

fn application_schema(schema: &DocumentSchemaNode) -> ProtocolPackageSchemaViewModel {
    ProtocolPackageSchemaViewModel {
        root: schema.clone(),
    }
}

pub(super) fn recent_error_view(
    reason: &PackageTransportError,
) -> ExternalPackageRecentErrorViewModel {
    let (code, message) = match reason {
        PackageTransportError::RegistrationDeadline => {
            ("EXTERNAL_PACKAGE_TIMEOUT", "外部软件包注册超过连接期限。")
        }
        PackageTransportError::Disconnected => {
            ("EXTERNAL_PACKAGE_DISCONNECTED", "外部软件包连接已断开。")
        }
        PackageTransportError::Remote { error, .. } => (
            error.data().code().as_str(),
            "外部软件包返回 JSON-RPC 错误。",
        ),
        PackageTransportError::MessageTooLarge { .. } => (
            "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE",
            "外部软件包消息超过限制。",
        ),
        PackageTransportError::Package { error } => (error.code.as_str(), "外部软件包合同无效。"),
        PackageTransportError::InvalidResponse => {
            ("EXTERNAL_PACKAGE_PROTOCOL_FATAL", "外部软件包响应无效。")
        }
        PackageTransportError::Transport(_) => {
            ("EXTERNAL_PACKAGE_TRANSPORT_ERROR", "外部软件包传输失败。")
        }
    };
    ExternalPackageRecentErrorViewModel {
        code: code.to_owned(),
        message: message.to_owned(),
        occurred_at: Utc::now(),
    }
}
