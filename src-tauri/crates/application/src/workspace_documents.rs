//! 可移植 Workspace 文档的唯一编解码边界。
//!
//! 内存仓储、SQLite 适配器、桌面 UI 和未来 CLI/TUI 都调用这里，避免不同入口对
//! 敏感字段、证书引用和文档大小采用不同规则。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    AppError, AppResult, PortableCertificateMaterial, ProxyWorkspace, ProxyWorkspaceV2,
    document_security::is_secret_field, validate_certificate_materials,
    validate_portable_certificate_references,
};

pub const WORKSPACE_DOCUMENT_FORMAT_VERSION: u16 = 3;
pub const WORKSPACE_DOCUMENT_V2_FORMAT_VERSION: u16 = 2;
pub const MAX_WORKSPACE_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocument {
    pub format_version: u16,
    pub workspace: ProxyWorkspace,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocumentV2 {
    pub format_version: u16,
    pub workspace: ProxyWorkspaceV2,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
}

impl TryFrom<WorkspaceDocumentV2> for WorkspaceDocument {
    type Error = AppError;

    fn try_from(value: WorkspaceDocumentV2) -> Result<Self, Self::Error> {
        if value.format_version != WORKSPACE_DOCUMENT_V2_FORMAT_VERSION {
            return Err(unsupported_version(value.format_version));
        }
        Ok(Self {
            format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace: value.workspace.into(),
            certificate_materials: value.certificate_materials,
        })
    }
}

impl WorkspaceDocument {
    pub fn validate(&self) -> AppResult<()> {
        if self.format_version != WORKSPACE_DOCUMENT_FORMAT_VERSION {
            return Err(AppError::new(
                "WORKSPACE_DOCUMENT_VERSION_UNSUPPORTED",
                format!(
                    "Workspace 文档版本 {} 不受支持；当前仅支持版本 {}。",
                    self.format_version, WORKSPACE_DOCUMENT_FORMAT_VERSION
                ),
            ));
        }
        self.workspace.validate().map_err(AppError::from)?;
        validate_portable_certificate_references(&self.workspace)?;
        validate_certificate_materials(
            std::slice::from_ref(&self.workspace),
            &self.certificate_materials,
        )
    }
}

/// 解析并验证可移植 Workspace，但不持久化也不重映射领域 ID。
pub fn parse_workspace_document(document: &[u8]) -> AppResult<WorkspaceDocument> {
    if document.len() > MAX_WORKSPACE_DOCUMENT_BYTES {
        return Err(AppError::new(
            "IMPORT_FAILED",
            "Workspace 文档超过 64 MiB 安全上限。",
        ));
    }
    let value = serde_json::from_slice::<Value>(document)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("Workspace JSON 无效：{error}")))?;
    reject_workspace_fields_outside_certificate_materials(&value)?;
    let version = read_format_version(&value)?;
    let parsed = match version {
        WORKSPACE_DOCUMENT_FORMAT_VERSION => serde_json::from_value::<WorkspaceDocument>(value)
            .map_err(|error| invalid_workspace_structure(&error))?,
        WORKSPACE_DOCUMENT_V2_FORMAT_VERSION => {
            let legacy = serde_json::from_value::<WorkspaceDocumentV2>(value)
                .map_err(|error| invalid_workspace_structure(&error))?;
            legacy.try_into()?
        }
        _ => return Err(unsupported_version(version)),
    };
    parsed.validate()?;
    Ok(parsed)
}

fn read_format_version(value: &Value) -> AppResult<u16> {
    value
        .get("format_version")
        .and_then(Value::as_u64)
        .and_then(|version| u16::try_from(version).ok())
        .ok_or_else(|| AppError::new("IMPORT_FAILED", "Workspace format_version 缺失或无效。"))
}

fn invalid_workspace_structure(error: &serde_json::Error) -> AppError {
    AppError::new("IMPORT_FAILED", format!("Workspace 结构无效：{error}"))
}

fn unsupported_version(version: u16) -> AppError {
    AppError::new(
        "WORKSPACE_DOCUMENT_VERSION_UNSUPPORTED",
        format!(
            "Workspace 文档版本 {version} 不受支持；当前支持版本 2 和 \
             {WORKSPACE_DOCUMENT_FORMAT_VERSION}。"
        ),
    )
}

/// 序列化经过领域校验的 Workspace，并对输出再次执行敏感字段扫描。
pub fn serialize_workspace_document(document: &WorkspaceDocument) -> AppResult<Vec<u8>> {
    document.validate()?;
    let document = serde_json::to_vec_pretty(document).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 序列化失败：{error}"))
    })?;
    let value = serde_json::from_slice::<Value>(&document).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 导出自检失败：{error}"))
    })?;
    reject_workspace_fields_outside_certificate_materials(&value)
        .map_err(|_| AppError::new("EXPORT_FAILED", "Workspace 包含禁止导出的敏感字段。"))?;
    Ok(document)
}

fn reject_workspace_fields_outside_certificate_materials(value: &Value) -> AppResult<()> {
    let mut scanned = value.clone();
    if let Some(object) = scanned.as_object_mut() {
        object.insert("certificate_materials".into(), Value::Array(Vec::new()));
    }
    reject_sensitive_workspace_fields(&scanned, "$")
}

fn reject_sensitive_workspace_fields(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if is_secret_field(key) {
                    return Err(AppError::new(
                        "IMPORT_FAILED",
                        format!("Workspace 文档包含禁止的敏感字段：{path}.{key}"),
                    ));
                }
                reject_sensitive_workspace_fields(value, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                reject_sensitive_workspace_fields(value, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{
        BodyCodecKind, CertificateReference, CertificateReferenceId, CertificateReferenceKind,
        DownstreamClientAuthentication, DownstreamTlsSettings, FixedServerSettings,
        ForwardProxyAuthentication, MitmSettings, ProxyListenerV2, ProxyWorkspaceV2,
        SecretReference, UpstreamTlsSettings,
    };
    use serde_json::json;

    use super::*;

    fn material(
        reference_id: CertificateReferenceId,
        label: &str,
        kind: CertificateReferenceKind,
        material_base64: &str,
        material_sha256: &str,
    ) -> PortableCertificateMaterial {
        PortableCertificateMaterial {
            reference_id,
            label: label.into(),
            kind,
            material_base64: material_base64.into(),
            material_sha256: material_sha256.into(),
            password: None,
        }
    }

    struct FixtureIds {
        root: CertificateReferenceId,
        server_identity: CertificateReferenceId,
        client_trust: CertificateReferenceId,
        server_trust: CertificateReferenceId,
        client_identity: CertificateReferenceId,
    }

    impl FixtureIds {
        fn new() -> Self {
            Self {
                root: CertificateReferenceId::new(),
                server_identity: CertificateReferenceId::new(),
                client_trust: CertificateReferenceId::new(),
                server_trust: CertificateReferenceId::new(),
                client_identity: CertificateReferenceId::new(),
            }
        }
    }

    fn checked_v2_listener(
        id: intercept_proxy_domain::ListenerId,
        ids: &FixtureIds,
    ) -> ProxyListenerV2 {
        ProxyListenerV2 {
            id,
            name: "v2 全字段 HTTP".into(),
            enabled: false,
            bind_address: "127.0.0.1".into(),
            port: 16_127,
            authentication: ForwardProxyAuthentication::Basic {
                credential: SecretReference {
                    provider: "keychain".into(),
                    key: "v2-basic".into(),
                },
            },
            allowed_client_cidrs: vec!["10.0.0.0/8".into()],
            mitm: MitmSettings {
                enabled: true,
                authority_allowlist: vec!["*.example.test".into()],
                root_ca: Some(ids.root),
                maximum_cached_leaf_certificates: 64,
            },
            connect_timeout_ms: 31_001,
            read_timeout_ms: 71_002,
            write_timeout_ms: 72_003,
            downstream_tls: Some(DownstreamTlsSettings {
                enabled: true,
                server_identity: Some(ids.server_identity),
                dynamic_sni_allowlist: vec!["api.example.test".into()],
                client_authentication: DownstreamClientAuthentication::Required {
                    trust: ids.client_trust,
                },
            }),
            request_body_codec: BodyCodecKind::ShiftJis,
            response_body_codec: BodyCodecKind::Utf8,
            fixed_server: Some(FixedServerSettings {
                upstream_url: "https://upstream.example.test:443".into(),
                upstream_tls: UpstreamTlsSettings {
                    verify_hostname: true,
                    server_trust: Some(ids.server_trust),
                    client_identity: Some(ids.client_identity),
                },
            }),
        }
    }

    fn checked_v2_references(ids: &FixtureIds) -> Vec<CertificateReference> {
        [
            (
                ids.root,
                "本机 Root",
                CertificateReferenceKind::MitmRootCa,
                "installation:root-ca",
            ),
            (
                ids.server_identity,
                "服务端身份",
                CertificateReferenceKind::ReverseServerIdentity,
                "managed:listener-tls:server",
            ),
            (
                ids.client_trust,
                "客户端信任",
                CertificateReferenceKind::DownstreamClientTrust,
                "managed:listener-tls:client-trust",
            ),
            (
                ids.server_trust,
                "上游信任",
                CertificateReferenceKind::UpstreamServerTrust,
                "managed:listener-tls:server-trust",
            ),
            (
                ids.client_identity,
                "上游身份",
                CertificateReferenceKind::UpstreamClientIdentity,
                "managed:listener-tls:client-identity",
            ),
        ]
        .into_iter()
        .map(|(id, label, kind, reference)| CertificateReference {
            id,
            label: label.into(),
            kind,
            reference: reference.into(),
        })
        .collect()
    }

    fn checked_v2_materials(ids: &FixtureIds) -> Vec<PortableCertificateMaterial> {
        vec![
            material(
                ids.server_identity,
                "服务端身份",
                CertificateReferenceKind::ReverseServerIdentity,
                "aWRlbnRpdHk=",
                "689f6a627384c7dcb2dcc1487e540223e77bdf9dcd0d8be8a326eda65b0ce9a4",
            ),
            material(
                ids.client_trust,
                "客户端信任",
                CertificateReferenceKind::DownstreamClientTrust,
                "Y2VydA==",
                "06298432e8066b29e2223bcc23aa9504b56ae508fabf3435508869b9c3190e22",
            ),
            material(
                ids.server_trust,
                "上游信任",
                CertificateReferenceKind::UpstreamServerTrust,
                "dHJ1c3Q=",
                "f796e2f28ae5811737ccb8233f34e09f8bb75d2511a135543d1ca37be0199a1d",
            ),
            material(
                ids.client_identity,
                "上游身份",
                CertificateReferenceKind::UpstreamClientIdentity,
                "Y2xpZW50",
                "948fe603f61dc036b5c596dc09fe3ce3f3d30dc90f024c85f3c82db2ccab679d",
            ),
        ]
    }

    fn checked_v2_document() -> WorkspaceDocumentV2 {
        let ids = FixtureIds::new();
        let workspace = ProxyWorkspace::default();
        WorkspaceDocumentV2 {
            format_version: WORKSPACE_DOCUMENT_V2_FORMAT_VERSION,
            workspace: ProxyWorkspaceV2 {
                id: workspace.id,
                name: "v2 fixture".into(),
                revision: workspace.revision,
                listeners: vec![checked_v2_listener(workspace.listeners[0].id, &ids)],
                metadata_extractors: Vec::new(),
                response_assertions: Vec::new(),
                rules: Vec::new(),
                fault_presets: Vec::new(),
                certificate_references: checked_v2_references(&ids),
                android_network_profiles: Vec::new(),
            },
            certificate_materials: checked_v2_materials(&ids),
        }
    }

    #[test]
    fn nested_camel_case_secret_is_rejected_before_serde_can_ignore_it() {
        let mut value = serde_json::to_value(ProxyWorkspace::default()).expect("workspace value");
        value["extension"] = json!({"credentials": {"privateKey": "forbidden"}});

        let error = parse_workspace_document(
            &serde_json::to_vec(&value).expect("workspace document bytes"),
        )
        .expect_err("unknown nested secret must be rejected");

        assert_eq!(error.view_model.code, "IMPORT_FAILED");
        assert!(error.view_model.message.contains("privateKey"));
    }

    #[test]
    fn v2_workspace_migrates_every_http_field_and_exports_v3() {
        let v2 = checked_v2_document();
        let expected_listener = v2.workspace.listeners[0].clone();
        let parsed = parse_workspace_document(&serde_json::to_vec(&v2).unwrap()).unwrap();

        assert_eq!(parsed.format_version, WORKSPACE_DOCUMENT_FORMAT_VERSION);
        assert_eq!(parsed.workspace.listeners[0], expected_listener.into());
        assert_eq!(
            parsed.workspace.certificate_references,
            v2.workspace.certificate_references
        );
        assert_eq!(parsed.certificate_materials, v2.certificate_materials);

        let exported = serialize_workspace_document(&parsed).unwrap();
        let exported_value: Value = serde_json::from_slice(&exported).unwrap();
        assert_eq!(exported_value["format_version"], 3);
        assert_eq!(
            exported_value["workspace"]["listeners"][0]["data_plane"]["kind"],
            "http"
        );
        assert_eq!(parse_workspace_document(&exported).unwrap(), parsed);
    }

    #[test]
    fn v2_minimal_historical_defaults_are_exact_and_unknown_fields_fail() {
        let mut v2 = checked_v2_document();
        v2.workspace.certificate_references.clear();
        v2.certificate_materials.clear();
        let mut value = serde_json::to_value(v2).unwrap();
        let listener = value["workspace"]["listeners"][0].as_object_mut().unwrap();
        listener["mitm"] = serde_json::to_value(MitmSettings::default()).unwrap();
        listener["authentication"] = serde_json::json!({"mode": "none"});
        listener["fixed_server"] = Value::Null;
        listener.remove("downstream_tls");
        listener.remove("request_body_codec");
        listener.remove("response_body_codec");
        value["workspace"]
            .as_object_mut()
            .unwrap()
            .remove("android_network_profiles");

        let parsed = parse_workspace_document(&serde_json::to_vec(&value).unwrap()).unwrap();
        let http = parsed.workspace.listeners[0].http().unwrap();
        assert_eq!(http.downstream_tls, DownstreamTlsSettings::default());
        assert_eq!(http.request_body_codec, BodyCodecKind::Auto);
        assert_eq!(http.response_body_codec, BodyCodecKind::Auto);
        assert!(parsed.workspace.android_network_profiles.is_empty());

        value["workspace"]["listeners"][0]["unknown_v2_field"] = json!(true);
        assert!(parse_workspace_document(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn v3_socket_workspace_round_trip_preserves_the_tagged_variant() {
        let listener = intercept_proxy_domain::ProxyListener {
            data_plane: intercept_proxy_domain::ListenerDataPlane::Socket(
                intercept_proxy_domain::SocketRelaySettings {
                    upstream: intercept_proxy_domain::SocketEndpoint {
                        host: "socket.example.test".into(),
                        port: 16_127,
                    },
                    security: intercept_proxy_domain::SocketRelaySecurity::Transparent,
                    maximum_connections: 777,
                    processing: intercept_proxy_domain::SocketPayloadProcessing::Scripted(
                        intercept_proxy_domain::ScriptedSocketProcessing {
                            package: intercept_proxy_domain::ProtocolPackageRef {
                                id: intercept_proxy_domain::ProtocolPackageId::new(
                                    "iso8583-standard",
                                )
                                .unwrap(),
                                version: intercept_proxy_domain::ProtocolPackageVersion::new(
                                    "1.2.3",
                                )
                                .unwrap(),
                            },
                            upstream: intercept_proxy_domain::DirectionProcessingOptions {
                                decode_enabled: true,
                                encode_enabled: false,
                            },
                            downstream: intercept_proxy_domain::DirectionProcessingOptions {
                                decode_enabled: false,
                                encode_enabled: true,
                            },
                        },
                    ),
                },
            ),
            ..intercept_proxy_domain::ProxyListener::default()
        };
        let workspace = ProxyWorkspace {
            listeners: vec![listener],
            ..ProxyWorkspace::default()
        };
        let document = WorkspaceDocument {
            format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace,
            certificate_materials: Vec::new(),
        };

        let bytes = serialize_workspace_document(&document).unwrap();
        let parsed = parse_workspace_document(&bytes).unwrap();
        assert_eq!(parsed, document);
        let socket = parsed.workspace.listeners[0].socket().unwrap();
        let intercept_proxy_domain::SocketPayloadProcessing::Scripted(processing) =
            &socket.processing
        else {
            panic!("scripted processing must survive workspace export/import")
        };
        assert!(processing.upstream.decode_enabled);
        assert!(!processing.upstream.encode_enabled);
        assert!(!processing.downstream.decode_enabled);
        assert!(processing.downstream.encode_enabled);
    }
}
