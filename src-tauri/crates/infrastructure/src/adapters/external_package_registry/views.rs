//! 外部协议包领域注册合同到应用查询模型的纯映射。

use chrono::Utc;
use intercept_proxy_application::{
    ExternalPackageDetailViewModel, ExternalPackageDirectionMethodsViewModel,
    ExternalPackageRecentErrorViewModel, ProtocolPackageCapabilitiesViewModel,
    ProtocolPackageDescriptionViewModel, ProtocolPackageDirectionCapabilitiesViewModel,
    ProtocolPackageKindViewModel, ProtocolPackageSchemaFieldTypeViewModel,
    ProtocolPackageSchemaFieldViewModel, ProtocolPackageSchemaViewModel,
    ProtocolPackageSourceViewModel, ProtocolPackageValidationViewModel,
    ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::{
    DocumentFieldType, DocumentSchema, ExternalPackageDirection, ExternalPackageMethodNamespace,
    ExternalPackageRegistration,
};

use super::ConnectionDetailSnapshot;
use crate::adapters::external_packages::ExternalPackageConnectionError;
use crate::sqlite::external_packages::StoredExternalPackage;

pub(super) fn application_summary(
    stored: &StoredExternalPackage,
    online: bool,
) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        package: stored.registration.package().identity().clone(),
        name: stored.registration.package().name().to_owned(),
        host_api: stored.registration.api(),
        kind: ProtocolPackageKindViewModel::Socket,
        source: ProtocolPackageSourceViewModel::External { online },
        enabled: stored.enabled,
        validation: ProtocolPackageValidationViewModel::Valid,
        installed_at: stored.first_connected_at,
    }
}

pub(super) fn application_description(
    registration: &ExternalPackageRegistration,
) -> ProtocolPackageDescriptionViewModel {
    let capabilities = ProtocolPackageDirectionCapabilitiesViewModel {
        frame: true,
        decode: true,
        encode: true,
    };
    ProtocolPackageDescriptionViewModel {
        package: registration.package().identity().clone(),
        kind: ProtocolPackageKindViewModel::Socket,
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: capabilities,
            downstream: capabilities,
            display: true,
        },
        upstream_schema: application_schema(registration.document().upstream().schema()),
        downstream_schema: application_schema(registration.document().downstream().schema()),
    }
}

pub(super) fn application_detail(
    stored: &StoredExternalPackage,
    connection: Option<&ConnectionDetailSnapshot>,
    online: bool,
) -> ExternalPackageDetailViewModel {
    let registration = &stored.registration;
    ExternalPackageDetailViewModel {
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
        rpc_timeout_seconds: connection.map_or(5, |detail| detail.rpc_timeout.as_secs()),
        upstream_methods: direction_methods(registration, ExternalPackageDirection::Upstream),
        downstream_methods: direction_methods(registration, ExternalPackageDirection::Downstream),
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

fn direction_methods(
    registration: &ExternalPackageRegistration,
    direction: ExternalPackageDirection,
) -> ExternalPackageDirectionMethodsViewModel {
    let (hooks, document) = match direction {
        ExternalPackageDirection::Upstream => (
            registration.hooks().upstream(),
            registration.document().upstream(),
        ),
        ExternalPackageDirection::Downstream => (
            registration.hooks().downstream(),
            registration.document().downstream(),
        ),
    };
    ExternalPackageDirectionMethodsViewModel {
        frame: hooks
            .frame()
            .qualified(ExternalPackageMethodNamespace::Hooks, direction),
        decode: hooks
            .decode()
            .qualified(ExternalPackageMethodNamespace::Hooks, direction),
        encode: hooks
            .encode()
            .qualified(ExternalPackageMethodNamespace::Hooks, direction),
        display: document
            .display()
            .qualified(ExternalPackageMethodNamespace::Document, direction),
    }
}

fn application_schema(schema: &DocumentSchema) -> ProtocolPackageSchemaViewModel {
    ProtocolPackageSchemaViewModel {
        id: schema.id().as_str().to_owned(),
        version: schema.version(),
        title: schema.title().to_owned(),
        fields: schema
            .fields()
            .iter()
            .map(|field| ProtocolPackageSchemaFieldViewModel {
                name: field.name().as_str().to_owned(),
                label: field.label().to_owned(),
                field_type: match field.field_type() {
                    DocumentFieldType::String => ProtocolPackageSchemaFieldTypeViewModel::String,
                    DocumentFieldType::Int => ProtocolPackageSchemaFieldTypeViewModel::Int,
                    DocumentFieldType::Bool => ProtocolPackageSchemaFieldTypeViewModel::Bool,
                    DocumentFieldType::Blob => ProtocolPackageSchemaFieldTypeViewModel::Blob,
                },
            })
            .collect(),
    }
}

pub(super) fn recent_error_view(
    reason: &ExternalPackageConnectionError,
) -> ExternalPackageRecentErrorViewModel {
    let (code, message) = match reason {
        ExternalPackageConnectionError::Busy => ("EXTERNAL_PACKAGE_BUSY", "外部软件包繁忙。"),
        ExternalPackageConnectionError::Timeout { .. } => {
            ("EXTERNAL_PACKAGE_TIMEOUT", "外部软件包调用超时。")
        }
        ExternalPackageConnectionError::Disconnected => {
            ("EXTERNAL_PACKAGE_DISCONNECTED", "外部软件包连接已断开。")
        }
        ExternalPackageConnectionError::Remote { .. } => (
            "EXTERNAL_PACKAGE_REMOTE_ERROR",
            "外部软件包返回 JSON-RPC 错误。",
        ),
        ExternalPackageConnectionError::MessageTooLarge { .. } => (
            "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE",
            "外部软件包消息超过限制。",
        ),
        ExternalPackageConnectionError::InvalidPayload(_) => (
            "EXTERNAL_PACKAGE_INVALID_PAYLOAD",
            "外部软件包 payload 无效。",
        ),
        ExternalPackageConnectionError::Fatal(_) => {
            ("EXTERNAL_PACKAGE_PROTOCOL_FATAL", "外部软件包协议失效。")
        }
        ExternalPackageConnectionError::Transport(_) => {
            ("EXTERNAL_PACKAGE_TRANSPORT_ERROR", "外部软件包传输失败。")
        }
    };
    ExternalPackageRecentErrorViewModel {
        code: code.to_owned(),
        message: message.to_owned(),
        occurred_at: Utc::now(),
    }
}
