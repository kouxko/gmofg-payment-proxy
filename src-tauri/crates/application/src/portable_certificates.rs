//! 单文件配置中的可移植 Listener TLS 材料。
//!
//! Workspace 运行时仍只保存 `managed:listener-tls:*` 引用；该结构仅存在于用户主动
//! 导入或导出的 JSON 文档中。导入后 infrastructure 会校验证书格式、写入当前系统的
//! 受保护存储，并用新的本机托管引用替换文档中的旧引用。

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Write as _},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    DownstreamClientAuthentication, ListenerDataPlane, ProxyWorkspace, SocketRelaySecurity,
};
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{AppError, AppResult};

/// 单份证书材料编码后的最大字符数，约等于 16 MiB 原始文件的 Base64 大小。
const MAX_ENCODED_MATERIAL_CHARS: usize = 24 * 1024 * 1024;
const MAX_PASSWORD_BYTES: usize = 16 * 1024;

#[derive(Clone, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PortableCertificateMaterial {
    pub reference_id: CertificateReferenceId,
    pub label: String,
    pub kind: CertificateReferenceKind,
    /// DER、PEM 或 PKCS#12 原始字节的 Base64 表示。
    pub material_base64: String,
    /// 原始证书材料的 SHA-256；导入前先校验，防止配置文件损坏或被截断。
    pub material_sha256: String,
    /// 仅 PKCS#12 使用；测试配置按用户要求允许明文携带。
    pub password: Option<String>,
}

// 证书原文和测试密码允许写入用户主动导出的 JSON，但绝不能意外进入 Rust 日志。
impl fmt::Debug for PortableCertificateMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortableCertificateMaterial")
            .field("reference_id", &self.reference_id)
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("material_bytes_base64_len", &self.material_base64.len())
            .field("material_sha256", &self.material_sha256)
            .field("password_present", &self.password.is_some())
            .finish()
    }
}

impl PortableCertificateMaterial {
    pub fn validate_shape(&self) -> AppResult<()> {
        if self.label.trim().is_empty()
            || self.material_base64.is_empty()
            || self.material_base64.len() > MAX_ENCODED_MATERIAL_CHARS
            || self
                .password
                .as_ref()
                .is_some_and(|value| value.len() > MAX_PASSWORD_BYTES)
        {
            return Err(AppError::new(
                "PORTABLE_CERTIFICATE_INVALID",
                "配置文件中的证书材料为空或超过允许大小。",
            )
            .entity(self.reference_id.to_string()));
        }
        let bytes = STANDARD.decode(&self.material_base64).map_err(|_| {
            AppError::new(
                "PORTABLE_CERTIFICATE_INVALID",
                "配置文件中的证书材料不是有效的 Base64。",
            )
            .entity(self.reference_id.to_string())
        })?;
        if bytes.is_empty() || self.material_sha256 != portable_material_sha256(&bytes) {
            return Err(AppError::new(
                "PORTABLE_CERTIFICATE_HASH_MISMATCH",
                "配置文件中的证书材料完整性校验失败。",
            )
            .entity(self.reference_id.to_string()));
        }
        Ok(())
    }
}

/// 统一生成单文件配置使用的证书材料摘要。
#[must_use]
pub fn portable_material_sha256(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            // 写入 String 不会失败；保留 Result 仅来自 `fmt::Write` 的统一接口。
            write!(output, "{byte:02x}").expect("writing SHA-256 to String cannot fail");
            output
        })
}

/// 只保留 Listener 实际使用的证书引用。
///
/// 旧版 Workspace 可能遗留已脱离 Listener 的文件路径或托管引用。它们不是
/// 当前配置的一部分，也不应该让已丢失的临时文件阻断导出。
pub fn retain_reachable_certificate_references(workspace: &mut ProxyWorkspace) {
    let mut reachable = BTreeSet::new();
    for listener in &workspace.listeners {
        match &listener.data_plane {
            ListenerDataPlane::Http(settings) => {
                reachable.extend(settings.mitm.root_ca);
                reachable.extend(settings.downstream_tls.server_identity);
                collect_client_trust(
                    &settings.downstream_tls.client_authentication,
                    &mut reachable,
                );
                if let Some(fixed_server) = &settings.fixed_server {
                    reachable.extend(fixed_server.upstream_tls.server_trust);
                    reachable.extend(fixed_server.upstream_tls.client_identity);
                }
            }
            ListenerDataPlane::Socket(settings) => match &settings.security {
                SocketRelaySecurity::Transparent => {}
                SocketRelaySecurity::TcpToTls { upstream_tls } => {
                    collect_socket_upstream(upstream_tls, &mut reachable);
                }
                SocketRelaySecurity::TlsToTcp { downstream_tls } => {
                    collect_socket_downstream(downstream_tls, &mut reachable);
                }
                SocketRelaySecurity::TlsToTls {
                    downstream_tls,
                    upstream_tls,
                } => {
                    collect_socket_downstream(downstream_tls, &mut reachable);
                    collect_socket_upstream(upstream_tls, &mut reachable);
                }
            },
        }
    }
    workspace
        .certificate_references
        .retain(|reference| reachable.contains(&reference.id));
}

fn collect_client_trust(
    authentication: &DownstreamClientAuthentication,
    reachable: &mut BTreeSet<CertificateReferenceId>,
) {
    match authentication {
        DownstreamClientAuthentication::Disabled => {}
        DownstreamClientAuthentication::Optional { trust }
        | DownstreamClientAuthentication::Required { trust } => {
            reachable.insert(*trust);
        }
    }
}

fn collect_socket_downstream(
    settings: &intercept_proxy_domain::SocketDownstreamTlsSettings,
    reachable: &mut BTreeSet<CertificateReferenceId>,
) {
    reachable.insert(settings.server_identity);
    collect_client_trust(&settings.client_authentication, reachable);
}

fn collect_socket_upstream(
    settings: &intercept_proxy_domain::SocketUpstreamTlsSettings,
    reachable: &mut BTreeSet<CertificateReferenceId>,
) {
    reachable.extend(settings.server_trust);
    reachable.extend(settings.client_identity);
}

/// 确保证书材料与 Workspace 中的引用一一对应。
///
/// 这一步只校验文档关系；证书链、私钥和 PKCS#12 密码由 infrastructure 在恢复时解析。
pub fn validate_certificate_materials(
    workspaces: &[ProxyWorkspace],
    materials: &[PortableCertificateMaterial],
) -> AppResult<()> {
    let mut references = BTreeMap::<CertificateReferenceId, &CertificateReference>::new();
    for reference in workspaces
        .iter()
        .flat_map(|workspace| workspace.certificate_references.iter())
    {
        if let Some(existing) = references.get(&reference.id) {
            if *existing != reference {
                return Err(AppError::new(
                    "PORTABLE_CERTIFICATE_INVALID",
                    "多个 Workspace 使用了冲突的证书引用 ID。",
                )
                .entity(reference.id.to_string()));
            }
        } else {
            references.insert(reference.id, reference);
        }
    }
    let portable_references = references
        .iter()
        .filter(|(_, reference)| reference.kind != CertificateReferenceKind::MitmRootCa)
        .map(|(id, reference)| (*id, *reference))
        .collect::<BTreeMap<_, _>>();

    let mut material_ids = BTreeSet::new();
    for material in materials {
        material.validate_shape()?;
        if !material_ids.insert(material.reference_id) {
            return Err(AppError::new(
                "PORTABLE_CERTIFICATE_INVALID",
                "配置文件中的证书材料 ID 不能重复。",
            )
            .entity(material.reference_id.to_string()));
        }
        let Some(reference) = portable_references.get(&material.reference_id) else {
            return Err(AppError::new(
                "PORTABLE_CERTIFICATE_INVALID",
                "配置文件包含未被任何 Workspace 引用或禁止导出的证书材料。",
            )
            .entity(material.reference_id.to_string()));
        };
        if material.label != reference.label || material.kind != reference.kind {
            return Err(AppError::new(
                "PORTABLE_CERTIFICATE_INVALID",
                "证书材料的名称或用途与 Workspace 引用不一致。",
            )
            .entity(material.reference_id.to_string()));
        }
    }

    if portable_references.len() != material_ids.len() {
        return Err(AppError::new(
            "PORTABLE_CERTIFICATE_MISSING",
            "配置文件没有包含全部 Listener TLS 证书材料。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_reference_can_be_shared_by_multiple_workspaces() {
        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "共享上游 CA".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "managed:listener-tls:test".into(),
        };
        let mut first = ProxyWorkspace::default();
        first.certificate_references.push(reference.clone());
        let mut second = ProxyWorkspace::default();
        second.certificate_references.push(reference.clone());
        let material = PortableCertificateMaterial {
            reference_id: reference.id,
            label: reference.label,
            kind: reference.kind,
            material_base64: "Y2VydA==".into(),
            material_sha256: portable_material_sha256(b"cert"),
            password: None,
        };

        validate_certificate_materials(&[first, second], &[material]).unwrap();
    }

    #[test]
    fn conflicting_shared_reference_is_rejected() {
        let id = CertificateReferenceId::new();
        let mut first = ProxyWorkspace::default();
        first.certificate_references.push(CertificateReference {
            id,
            label: "CA A".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "managed:listener-tls:a".into(),
        });
        let mut second = ProxyWorkspace::default();
        second.certificate_references.push(CertificateReference {
            id,
            label: "CA B".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "managed:listener-tls:b".into(),
        });

        let error = validate_certificate_materials(&[first, second], &[]).unwrap_err();
        assert_eq!(error.view_model.code, "PORTABLE_CERTIFICATE_INVALID");
    }

    #[test]
    fn installation_root_reference_requires_no_portable_material() {
        let mut workspace = ProxyWorkspace::default();
        workspace.certificate_references.push(CertificateReference {
            id: CertificateReferenceId::new(),
            label: "本机 MITM Root CA".into(),
            kind: CertificateReferenceKind::MitmRootCa,
            reference: "installation:root-ca".into(),
        });

        validate_certificate_materials(&[workspace], &[]).unwrap();
    }

    #[test]
    fn installation_root_material_is_rejected() {
        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "本机 MITM Root CA".into(),
            kind: CertificateReferenceKind::MitmRootCa,
            reference: "installation:root-ca".into(),
        };
        let mut workspace = ProxyWorkspace::default();
        workspace.certificate_references.push(reference.clone());
        let material = PortableCertificateMaterial {
            reference_id: reference.id,
            label: reference.label,
            kind: reference.kind,
            material_base64: STANDARD.encode(b"must-not-export"),
            material_sha256: portable_material_sha256(b"must-not-export"),
            password: None,
        };

        let error = validate_certificate_materials(&[workspace], &[material]).unwrap_err();
        assert_eq!(error.view_model.code, "PORTABLE_CERTIFICATE_INVALID");
    }

    #[test]
    fn debug_output_redacts_certificate_material_and_password() {
        let material = PortableCertificateMaterial {
            reference_id: CertificateReferenceId::new(),
            label: "测试身份".into(),
            kind: CertificateReferenceKind::UpstreamClientIdentity,
            material_base64: "c2Vuc2l0aXZlLXBheWxvYWQ=".into(),
            material_sha256: portable_material_sha256(b"sensitive-payload"),
            password: Some("plain-test-password".into()),
        };

        let debug = format!("{material:?}");
        assert!(!debug.contains("c2Vuc2l0aXZlLXBheWxvYWQ="));
        assert!(!debug.contains("plain-test-password"));
        assert!(debug.contains("password_present: true"));
    }

    #[test]
    fn altered_certificate_material_is_rejected_before_restore() {
        let material = PortableCertificateMaterial {
            reference_id: CertificateReferenceId::new(),
            label: "测试 CA".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            material_base64: STANDARD.encode(b"changed"),
            material_sha256: portable_material_sha256(b"original"),
            password: None,
        };

        let error = material.validate_shape().unwrap_err();
        assert_eq!(error.view_model.code, "PORTABLE_CERTIFICATE_HASH_MISMATCH");
    }

    #[test]
    fn socket_bridge_references_are_reachable_but_transparent_uses_none() {
        let server_identity = CertificateReferenceId::new();
        let client_trust = CertificateReferenceId::new();
        let server_trust = CertificateReferenceId::new();
        let client_identity = CertificateReferenceId::new();
        let orphan = CertificateReferenceId::new();
        let mut workspace = ProxyWorkspace::default();
        workspace.listeners[0].data_plane =
            ListenerDataPlane::Socket(intercept_proxy_domain::SocketRelaySettings {
                upstream: intercept_proxy_domain::SocketEndpoint {
                    host: "socket.example.test".into(),
                    port: 443,
                },
                security: SocketRelaySecurity::TlsToTls {
                    downstream_tls: intercept_proxy_domain::SocketDownstreamTlsSettings {
                        server_identity,
                        client_authentication: DownstreamClientAuthentication::Required {
                            trust: client_trust,
                        },
                    },
                    upstream_tls: intercept_proxy_domain::SocketUpstreamTlsSettings {
                        verify_hostname: true,
                        server_trust: Some(server_trust),
                        client_identity: Some(client_identity),
                    },
                },
                maximum_connections: 500,
                processing: intercept_proxy_domain::SocketPayloadProcessing::Direct,
            });
        workspace.certificate_references = [
            (
                server_identity,
                CertificateReferenceKind::ReverseServerIdentity,
            ),
            (
                client_trust,
                CertificateReferenceKind::DownstreamClientTrust,
            ),
            (server_trust, CertificateReferenceKind::UpstreamServerTrust),
            (
                client_identity,
                CertificateReferenceKind::UpstreamClientIdentity,
            ),
            (orphan, CertificateReferenceKind::UpstreamServerTrust),
        ]
        .into_iter()
        .map(|(id, kind)| CertificateReference {
            id,
            label: id.to_string(),
            kind,
            reference: format!("managed:listener-tls:{id}"),
        })
        .collect();

        retain_reachable_certificate_references(&mut workspace);
        let retained = workspace
            .certificate_references
            .iter()
            .map(|reference| reference.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            retained,
            BTreeSet::from([server_identity, client_trust, server_trust, client_identity])
        );

        let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
            unreachable!()
        };
        settings.security = SocketRelaySecurity::Transparent;
        retain_reachable_certificate_references(&mut workspace);
        assert!(workspace.certificate_references.is_empty());
    }
}
