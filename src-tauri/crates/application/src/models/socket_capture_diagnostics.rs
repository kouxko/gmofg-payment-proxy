//! Socket observer/opened 的轻量、脱敏 DTO。
//!
//! 完整 Document 和网络字节只属于持久化 capture。已有 Proxy `RequestParsed` 使用自己的
//! 双预算 preview，本模块不再定义第二套全量实时消息。

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketCaptureFailureStage {
    Frame,
    Decode,
    Rule,
    Encode,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 已脱敏的 capture 失败证据；不携带 Document、原始帧或部分 written bytes。
pub struct SocketCaptureFailureDiagnostic {
    pub stage: SocketCaptureFailureStage,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 上游 TLS 握手的结构化证据；不保存证书原文。
pub struct SocketTlsEvidenceViewModel {
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 一次上游连接测试的脱敏结果。
pub struct SocketConnectionTestEvidenceViewModel {
    pub resolved_address: String,
    pub transport: String,
    pub tls: Option<SocketTlsEvidenceViewModel>,
    pub elapsed_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "topology", rename_all = "snake_case")]
/// 连接建立时的模式化路由证据；LocalResponder 分支不存在可伪造的上游字段。
pub enum SocketConnectionRouteViewModel {
    Relay(Box<SocketRelayRouteEvidenceViewModel>),
    LocalResponder { downstream_tls_peer: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRelayRouteEvidenceViewModel {
    pub configured_address: Option<String>,
    pub resolved_address: Option<String>,
    pub downstream_tls_peer: Option<String>,
    pub upstream_tls: Option<SocketTlsEvidenceViewModel>,
    pub connection_test: Option<SocketConnectionTestEvidenceViewModel>,
}

impl<'de> Deserialize<'de> for SocketConnectionRouteViewModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LocalResponderFields {
            downstream_tls_peer: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(tag = "topology", rename_all = "snake_case")]
        enum Wire {
            Relay(Box<SocketRelayRouteEvidenceViewModel>),
            LocalResponder(LocalResponderFields),
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Relay(fields) => Self::Relay(fields),
            Wire::LocalResponder(fields) => Self::LocalResponder {
                downstream_tls_peer: fields.downstream_tls_peer,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn local_route_rejects_every_upstream_evidence_field() {
        for forged in [
            json!({"configured_address": "forged.example:443"}),
            json!({"resolved_address": "192.0.2.10:443"}),
            json!({"upstream_tls": null}),
            json!({"connection_test": null}),
        ] {
            let mut value = json!({
                "topology": "local_responder",
                "downstream_tls_peer": "sha256:client"
            });
            value
                .as_object_mut()
                .unwrap()
                .extend(forged.as_object().unwrap().clone());
            assert!(serde_json::from_value::<SocketConnectionRouteViewModel>(value).is_err());
        }
    }

    #[test]
    fn relay_route_round_trip_preserves_optional_evidence() {
        let route =
            SocketConnectionRouteViewModel::Relay(Box::new(SocketRelayRouteEvidenceViewModel {
                configured_address: Some("example.test:443".into()),
                resolved_address: Some("192.0.2.10:443".into()),
                downstream_tls_peer: Some("sha256:client".into()),
                upstream_tls: Some(SocketTlsEvidenceViewModel {
                    tls_version: "TLSv1.3".into(),
                    cipher_suite: "TLS_AES_256_GCM_SHA384".into(),
                    peer_subject: "CN=example.test".into(),
                    peer_sha256_fingerprint: "sha256:server".into(),
                    hostname_verification_enabled: true,
                    client_identity_configured: false,
                }),
                connection_test: Some(SocketConnectionTestEvidenceViewModel {
                    resolved_address: "192.0.2.10:443".into(),
                    transport: "tls".into(),
                    tls: None,
                    elapsed_millis: 12,
                }),
            }));
        let wire = serde_json::to_value(&route).unwrap();
        assert_eq!(
            serde_json::from_value::<SocketConnectionRouteViewModel>(wire).unwrap(),
            route
        );
    }
}
