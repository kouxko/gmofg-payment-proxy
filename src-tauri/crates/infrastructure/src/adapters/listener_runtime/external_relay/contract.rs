//! 外部 Socket 数据面的启动端口与不可变绑定。

use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    ExternalDecodeRequest, ExternalDecodeResponse, ExternalDisplayRequest, ExternalDisplayResponse,
    ExternalEncodeRequest, ExternalEncodeResponse, ExternalFrameRequest, ExternalFrameResult,
    ExternalPackageRegistration, ProtocolPackageRef, ProxyListener, ProxyWorkspace, SocketTopology,
};

use super::super::{ProtocolDocumentRuleConnectionFactory, scripted_snapshot};
use crate::adapters::external_packages::{ExternalPackageClient, ExternalPackageConnectionError};

/// 外部连接的协议入口窄接口。
#[async_trait]
pub(crate) trait ExternalPackageRpc: fmt::Debug + Send + Sync {
    async fn frame(
        &self,
        method: &str,
        request: &ExternalFrameRequest,
    ) -> Result<ExternalFrameResult, ExternalPackageConnectionError>;
    async fn decode(
        &self,
        method: &str,
        request: &ExternalDecodeRequest,
    ) -> Result<ExternalDecodeResponse, ExternalPackageConnectionError>;
    async fn encode(
        &self,
        method: &str,
        request: &ExternalEncodeRequest,
    ) -> Result<ExternalEncodeResponse, ExternalPackageConnectionError>;
    async fn display(
        &self,
        method: &str,
        request: &ExternalDisplayRequest,
    ) -> Result<ExternalDisplayResponse, ExternalPackageConnectionError>;
}

#[async_trait]
impl ExternalPackageRpc for ExternalPackageClient {
    async fn frame(
        &self,
        method: &str,
        request: &ExternalFrameRequest,
    ) -> Result<ExternalFrameResult, ExternalPackageConnectionError> {
        self.call(method, request).await
    }
    async fn decode(
        &self,
        method: &str,
        request: &ExternalDecodeRequest,
    ) -> Result<ExternalDecodeResponse, ExternalPackageConnectionError> {
        self.call(method, request).await
    }
    async fn encode(
        &self,
        method: &str,
        request: &ExternalEncodeRequest,
    ) -> Result<ExternalEncodeResponse, ExternalPackageConnectionError> {
        self.call(method, request).await
    }
    async fn display(
        &self,
        method: &str,
        request: &ExternalDisplayRequest,
    ) -> Result<ExternalDisplayResponse, ExternalPackageConnectionError> {
        self.call_display(method, request).await
    }
}

/// 注册快照与对应在线 actor 的不可分割绑定。
#[derive(Clone)]
pub(crate) struct ExternalSocketPackageBinding {
    pub(crate) registration: ExternalPackageRegistration,
    pub(crate) rpc: Arc<dyn ExternalPackageRpc>,
    max_frame_bytes: usize,
    rpc_timeout: Duration,
}

impl ExternalSocketPackageBinding {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new(
        registration: ExternalPackageRegistration,
        rpc: Arc<dyn ExternalPackageRpc>,
    ) -> Self {
        Self::with_limits(registration, rpc, 8 * 1024 * 1024, Duration::from_secs(5))
    }

    pub(crate) fn with_limits(
        registration: ExternalPackageRegistration,
        rpc: Arc<dyn ExternalPackageRpc>,
        max_frame_bytes: usize,
        rpc_timeout: Duration,
    ) -> Self {
        Self {
            registration,
            rpc,
            max_frame_bytes,
            rpc_timeout,
        }
    }
    pub(crate) const fn registration(&self) -> &ExternalPackageRegistration {
        &self.registration
    }
    pub(crate) const fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes
    }
    pub(crate) const fn rpc_timeout(&self) -> Duration {
        self.rpc_timeout
    }
}

impl fmt::Debug for ExternalSocketPackageBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalSocketPackageBinding")
            .field("package", self.registration.package().identity())
            .finish_non_exhaustive()
    }
}

/// Listener 启动阶段解析外部协议包的最小端口。
pub(crate) trait ExternalSocketPackageProvider: fmt::Debug + Send + Sync {
    fn resolve(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ExternalSocketPackageBinding>>;
}

/// 一次 Listener start 冻结的外部注册合同与可热替换规则集合。
#[derive(Clone)]
pub(crate) struct ExternalSocketRuntimeSnapshot {
    pub(crate) binding: ExternalSocketPackageBinding,
    pub(crate) rules: ProtocolDocumentRuleConnectionFactory,
    topology: SocketTopology,
}

impl ExternalSocketRuntimeSnapshot {
    pub(crate) fn new(
        binding: ExternalSocketPackageBinding,
        rules: ProtocolDocumentRuleConnectionFactory,
        topology: SocketTopology,
    ) -> Self {
        Self {
            binding,
            rules,
            topology,
        }
    }

    /// 复用现有编译边界，原子替换运行中外部 Listener 的四阶段规则。
    pub(crate) fn replace_document_rules(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<()> {
        let registration = self.binding.registration();
        let replacement = scripted_snapshot::compile_document_rules(
            workspace,
            listener,
            registration.package().identity(),
            registration.document().upstream().schema(),
            registration.document().downstream().schema(),
            &self.topology,
        )?;
        self.rules.replace(&replacement);
        Ok(())
    }
}

impl fmt::Debug for ExternalSocketRuntimeSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalSocketRuntimeSnapshot")
            .field("binding", &self.binding)
            .field("topology", &self.topology)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_domain::{
        DocumentAction, DocumentFieldName, DocumentValue, ExternalDocumentWire, ListenerId,
        ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId, ProtocolDocumentRuleProgram,
        ProtocolRuleStage, SocketLocalResponderTopology,
    };
    use serde_json::json;
    use uuid::Uuid;

    #[derive(Debug)]
    struct DisconnectedRpc;

    #[async_trait]
    impl ExternalPackageRpc for DisconnectedRpc {
        async fn frame(
            &self,
            _method: &str,
            _request: &ExternalFrameRequest,
        ) -> Result<ExternalFrameResult, ExternalPackageConnectionError> {
            Err(ExternalPackageConnectionError::Disconnected)
        }

        async fn decode(
            &self,
            _method: &str,
            _request: &ExternalDecodeRequest,
        ) -> Result<ExternalDecodeResponse, ExternalPackageConnectionError> {
            Err(ExternalPackageConnectionError::Disconnected)
        }

        async fn encode(
            &self,
            _method: &str,
            _request: &ExternalEncodeRequest,
        ) -> Result<ExternalEncodeResponse, ExternalPackageConnectionError> {
            Err(ExternalPackageConnectionError::Disconnected)
        }

        async fn display(
            &self,
            _method: &str,
            _request: &ExternalDisplayRequest,
        ) -> Result<ExternalDisplayResponse, ExternalPackageConnectionError> {
            Err(ExternalPackageConnectionError::Disconnected)
        }
    }

    #[tokio::test]
    async fn rpc_contract_frame_preserves_connection_failure() {
        let rpc = DisconnectedRpc;
        assert!(matches!(
            rpc.frame("frame", &ExternalFrameRequest::from_bytes(b"frame"))
                .await,
            Err(ExternalPackageConnectionError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn rpc_contract_decode_preserves_connection_failure() {
        let rpc = DisconnectedRpc;
        assert!(matches!(
            rpc.decode("decode", &ExternalDecodeRequest::from_bytes(b"frame"))
                .await,
            Err(ExternalPackageConnectionError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn rpc_contract_encode_preserves_connection_failure() {
        let rpc = DisconnectedRpc;
        assert!(matches!(
            rpc.encode(
                "encode",
                &ExternalEncodeRequest {
                    document: ExternalDocumentWire::default(),
                },
            )
            .await,
            Err(ExternalPackageConnectionError::Disconnected)
        ));
    }

    #[tokio::test]
    async fn rpc_contract_display_preserves_connection_failure() {
        let rpc = DisconnectedRpc;
        assert!(matches!(
            rpc.display(
                "display",
                &ExternalDisplayRequest {
                    document: ExternalDocumentWire::default(),
                },
            )
            .await,
            Err(ExternalPackageConnectionError::Disconnected)
        ));
    }

    #[test]
    fn binding_preserves_registration_limits_and_safe_debug_identity() {
        let registration = registration();
        let package = registration.package().identity().clone();
        let binding = ExternalSocketPackageBinding::with_limits(
            registration,
            Arc::new(DisconnectedRpc),
            4096,
            Duration::from_millis(250),
        );

        assert_eq!(binding.registration().package().identity(), &package);
        assert_eq!(binding.max_frame_bytes(), 4096);
        assert_eq!(binding.rpc_timeout(), Duration::from_millis(250));
        assert_eq!(
            format!("{binding:?}"),
            "ExternalSocketPackageBinding { package: ProtocolPackageRef { id: ProtocolPackageId(\"contract-test\"), version: ProtocolPackageVersion(\"1.0.0\") }, .. }"
        );
    }

    #[test]
    fn snapshot_debug_includes_binding_and_topology_without_rule_state() {
        let registration = registration();
        let listener = listener();
        let snapshot = ExternalSocketRuntimeSnapshot::new(
            ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
            empty_rules(&registration, listener.id),
            SocketTopology::default(),
        );

        let debug = format!("{snapshot:?}");

        assert!(debug.contains("ExternalSocketRuntimeSnapshot"));
        assert!(debug.contains("contract-test"));
        assert!(debug.contains("topology: Relay"));
        assert!(!debug.contains("rules"));
    }

    #[test]
    fn replace_document_rules_installs_new_rules_for_the_running_snapshot() {
        let registration = registration();
        let listener = listener();
        let package = registration.package().identity().clone();
        let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
            ProtocolDocumentRuleId::new(),
            "relay rule".to_owned(),
            true,
            10,
            1,
            listener.id,
            package,
            registration.document().upstream().schema().version(),
            ProtocolRuleStage::ProxyToUpstream,
            Vec::new(),
            vec![DocumentAction::SetField {
                field: DocumentFieldName::new("request").unwrap(),
                value: DocumentValue::String("updated".to_owned()),
            }],
        )
        .unwrap();
        let workspace = ProxyWorkspace {
            protocol_rules: vec![rule],
            ..ProxyWorkspace::default()
        };
        let snapshot = ExternalSocketRuntimeSnapshot::new(
            ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
            empty_rules(&registration, listener.id),
            SocketTopology::default(),
        );

        snapshot
            .replace_document_rules(&workspace, &listener)
            .unwrap();

        assert_eq!(
            snapshot
                .rules
                .program(ProtocolRuleStage::ProxyToUpstream)
                .rules()
                .len(),
            1
        );
    }

    #[test]
    fn replace_document_rules_rejects_relay_only_stage_for_local_responder() {
        let registration = registration();
        let listener = listener();
        let package = registration.package().identity().clone();
        let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
            ProtocolDocumentRuleId::new(),
            "invalid local stage".to_owned(),
            true,
            10,
            1,
            listener.id,
            package,
            registration.document().upstream().schema().version(),
            ProtocolRuleStage::ProxyToUpstream,
            Vec::new(),
            vec![DocumentAction::SetField {
                field: DocumentFieldName::new("request").unwrap(),
                value: DocumentValue::String("updated".to_owned()),
            }],
        )
        .unwrap();
        let workspace = ProxyWorkspace {
            protocol_rules: vec![rule],
            ..ProxyWorkspace::default()
        };
        let snapshot = ExternalSocketRuntimeSnapshot::new(
            ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
            empty_rules(&registration, listener.id),
            SocketTopology::LocalResponder(SocketLocalResponderTopology::default()),
        );

        let error = snapshot
            .replace_document_rules(&workspace, &listener)
            .unwrap_err();

        assert_eq!(error.view_model.code, "PROTOCOL_RULE_DIRECTION_INVALID");
        assert!(
            snapshot
                .rules
                .program(ProtocolRuleStage::ProxyToUpstream)
                .rules()
                .is_empty()
        );
    }

    fn listener() -> ProxyListener {
        ProxyListener {
            id: ListenerId::from_uuid(Uuid::from_u128(42)),
            ..ProxyListener::default()
        }
    }

    fn empty_rules(
        registration: &ExternalPackageRegistration,
        listener_id: ListenerId,
    ) -> ProtocolDocumentRuleConnectionFactory {
        let package = registration.package().identity().clone();
        let upstream = registration.document().upstream().schema().clone();
        let downstream = registration.document().downstream().schema().clone();
        let program = |stage, schema| {
            Arc::new(
                ProtocolDocumentRuleProgram::new_for_stage(
                    listener_id,
                    package.clone(),
                    schema,
                    stage,
                    Vec::new(),
                )
                .unwrap(),
            )
        };
        ProtocolDocumentRuleConnectionFactory::new(
            program(ProtocolRuleStage::AppToProxy, upstream.clone()),
            program(ProtocolRuleStage::ProxyToUpstream, upstream),
            program(ProtocolRuleStage::UpstreamToProxy, downstream.clone()),
            program(ProtocolRuleStage::ProxyToApp, downstream),
        )
        .unwrap()
    }

    fn registration() -> ExternalPackageRegistration {
        serde_json::from_value(json!({
            "api": 1,
            "package": {
                "id": "contract-test",
                "name": "Contract test",
                "version": "1.0.0",
                "description": "test"
            },
            "document": {
                "upstream": {
                    "schema": {
                        "id": "contract-upstream",
                        "title": "Upstream",
                        "version": 1,
                        "fields": [
                            {"name": "request", "label": "Request", "type": "string"}
                        ]
                    },
                    "display": "render"
                },
                "downstream": {
                    "schema": {
                        "id": "contract-downstream",
                        "title": "Downstream",
                        "version": 1,
                        "fields": [
                            {"name": "response", "label": "Response", "type": "string"}
                        ]
                    },
                    "display": "render"
                }
            },
            "hooks": {
                "upstream": {"frame": "frame", "decode": "decode", "encode": "encode"},
                "downstream": {"frame": "frame", "decode": "decode", "encode": "encode"}
            }
        }))
        .unwrap()
    }
}
