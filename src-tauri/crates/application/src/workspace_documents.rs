//! 可移植 Workspace 文档的唯一编解码边界。
//!
//! 内存仓储、SQLite 适配器、桌面 UI 和未来 CLI/TUI 都调用这里，避免不同入口对
//! 敏感字段、证书引用和文档大小采用不同规则。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    AppError, AppResult, MigrationReport, MigrationSourceKind, PortableCertificateMaterial,
    PortableProtocolPackage, ProxyWorkspace, document_security::is_secret_field,
    migrate_workspace_value, validate_certificate_materials,
    validate_portable_certificate_references, validate_workspace_package_references,
};

pub const WORKSPACE_DOCUMENT_FORMAT_VERSION: u16 = 5;
pub const WORKSPACE_DOCUMENT_V4_FORMAT_VERSION: u16 = 4;
pub const WORKSPACE_DOCUMENT_V3_FORMAT_VERSION: u16 = 3;
pub const WORKSPACE_DOCUMENT_V2_FORMAT_VERSION: u16 = 2;
pub const MAX_WORKSPACE_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocument {
    pub format_version: u16,
    pub workspace: ProxyWorkspace,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
    pub protocol_packages: Vec<PortableProtocolPackage>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkspaceDocument {
    pub format_version: u16,
    pub workspace: Value,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceDocumentV4 {
    pub format_version: u16,
    pub workspace: Value,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
    pub protocol_packages: Vec<PortableProtocolPackage>,
}

/// 已迁移到当前模型的 Workspace 文档及其原始 wire 版本。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedWorkspaceDocument {
    pub source_version: u16,
    pub migration_report: MigrationReport,
    pub document: WorkspaceDocument,
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
        self.validate_common()?;
        validate_workspace_package_references(&self.workspace, &self.protocol_packages, true)
    }

    fn validate_common(&self) -> AppResult<()> {
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
    Ok(parse_workspace_document_with_source(document)?.document)
}

/// 解析 Workspace 文档并保留来源版本，供导入用例区分缺少协议包载荷的 legacy 文档。
pub fn parse_workspace_document_with_source(document: &[u8]) -> AppResult<ParsedWorkspaceDocument> {
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
    let (parsed, migration_report) = match version {
        WORKSPACE_DOCUMENT_FORMAT_VERSION => {
            let document = serde_json::from_value::<WorkspaceDocument>(value)
                .map_err(|error| invalid_workspace_structure(&error))?;
            (
                document,
                MigrationReport::unchanged(MigrationSourceKind::WorkspaceDocument, version),
            )
        }
        WORKSPACE_DOCUMENT_V4_FORMAT_VERSION => {
            let legacy = serde_json::from_value::<WorkspaceDocumentV4>(value)
                .map_err(|error| invalid_workspace_structure(&error))?;
            let (workspace, report) = migrate_workspace_value(
                legacy.workspace,
                MigrationSourceKind::WorkspaceDocument,
                version,
            )?;
            (
                WorkspaceDocument {
                    format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
                    workspace,
                    certificate_materials: legacy.certificate_materials,
                    protocol_packages: legacy.protocol_packages,
                },
                report,
            )
        }
        WORKSPACE_DOCUMENT_V2_FORMAT_VERSION | WORKSPACE_DOCUMENT_V3_FORMAT_VERSION => {
            let legacy = serde_json::from_value::<LegacyWorkspaceDocument>(value)
                .map_err(|error| invalid_workspace_structure(&error))?;
            let (workspace, report) = migrate_workspace_value(
                legacy.workspace,
                MigrationSourceKind::WorkspaceDocument,
                version,
            )?;
            (
                WorkspaceDocument {
                    format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
                    workspace,
                    certificate_materials: legacy.certificate_materials,
                    protocol_packages: Vec::new(),
                },
                report,
            )
        }
        _ => return Err(unsupported_version(version)),
    };
    parsed.validate_common()?;
    validate_workspace_package_references(
        &parsed.workspace,
        &parsed.protocol_packages,
        version >= WORKSPACE_DOCUMENT_V4_FORMAT_VERSION,
    )?;
    Ok(ParsedWorkspaceDocument {
        source_version: version,
        migration_report,
        document: parsed,
    })
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
            "Workspace 文档版本 {version} 不受支持；当前支持版本 2、3、4 和 \
             {WORKSPACE_DOCUMENT_FORMAT_VERSION}。"
        ),
    )
}

/// 序列化经过领域校验的 Workspace，并对输出再次执行敏感字段扫描。
pub fn serialize_workspace_document(document: &WorkspaceDocument) -> AppResult<Vec<u8>> {
    document.validate()?;
    let value = serde_json::to_value(document).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 序列化失败：{error}"))
    })?;
    reject_workspace_fields_outside_certificate_materials(&value)
        .map_err(|_| AppError::new("EXPORT_FAILED", "Workspace 包含禁止导出的敏感字段。"))?;
    serde_json::to_vec_pretty(&value)
        .map_err(|error| AppError::new("EXPORT_FAILED", format!("Workspace 序列化失败：{error}")))
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
        ForwardProxyAuthentication, MitmSettings, ProxyListenerV2, SecretReference,
        UpstreamTlsSettings,
    };
    use serde_json::json;

    use super::*;

    mod socket_compatibility;

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

    fn checked_v2_document() -> Value {
        let ids = FixtureIds::new();
        let workspace = ProxyWorkspace::default();
        json!({
            "format_version": WORKSPACE_DOCUMENT_V2_FORMAT_VERSION,
            "workspace": {
                "id": workspace.id,
                "name": "v2 fixture",
                "revision": workspace.revision,
                "listeners": [checked_v2_listener(workspace.listeners[0].id, &ids)],
                "metadata_extractors": [],
                "response_assertions": [],
                "rules": [],
                "fault_presets": [],
                "certificate_references": checked_v2_references(&ids),
                "android_network_profiles": [],
            },
            "certificate_materials": checked_v2_materials(&ids),
        })
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
    fn v2_workspace_migrates_every_http_field_and_exports_v4() {
        let v2 = checked_v2_document();
        let expected_listener: ProxyListenerV2 =
            serde_json::from_value(v2["workspace"]["listeners"][0].clone()).unwrap();
        let expected_references: Vec<CertificateReference> =
            serde_json::from_value(v2["workspace"]["certificate_references"].clone()).unwrap();
        let expected_materials: Vec<PortableCertificateMaterial> =
            serde_json::from_value(v2["certificate_materials"].clone()).unwrap();
        let parsed = parse_workspace_document(&serde_json::to_vec(&v2).unwrap()).unwrap();

        assert_eq!(parsed.format_version, WORKSPACE_DOCUMENT_FORMAT_VERSION);
        assert_eq!(parsed.workspace.listeners[0], expected_listener.into());
        assert_eq!(parsed.workspace.certificate_references, expected_references);
        assert_eq!(parsed.certificate_materials, expected_materials);
        assert!(parsed.protocol_packages.is_empty());

        let exported = serialize_workspace_document(&parsed).unwrap();
        let exported_value: Value = serde_json::from_slice(&exported).unwrap();
        assert_eq!(exported_value["format_version"], 5);
        assert_eq!(
            exported_value["workspace"]["listeners"][0]["data_plane"]["kind"],
            "http"
        );
        assert_eq!(parse_workspace_document(&exported).unwrap(), parsed);
    }

    #[test]
    fn v2_minimal_historical_defaults_are_exact_and_unknown_fields_fail() {
        let mut v2 = checked_v2_document();
        v2["workspace"]["certificate_references"] = json!([]);
        v2["certificate_materials"] = json!([]);
        let mut value = v2;
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
    fn outer_size_limit_counts_embedded_rule_and_package_bytes_before_json_parsing() {
        let mut document = br#"{"format_version":4,"workspace":{"socket_rules":["#.to_vec();
        document.resize(MAX_WORKSPACE_DOCUMENT_BYTES + 1, b'x');

        let error = parse_workspace_document(&document).expect_err("oversized document rejected");
        assert_eq!(error.view_model.code, "IMPORT_FAILED");
        assert!(error.view_model.message.contains("64 MiB"));
    }
}
