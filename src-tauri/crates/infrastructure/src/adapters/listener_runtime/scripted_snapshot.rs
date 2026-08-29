//! Scripted Socket Listener 的不可变启动快照。

use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, DownstreamClientAuthentication,
    ProtocolDirection as RuleDirection, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleProgram, ProtocolRuleStage, ProxyListener, ProxyWorkspace,
    SocketDownstreamSecurity, SocketDownstreamTlsSettings, SocketPayloadProcessing,
    SocketRelaySecurity as DomainSecurity, SocketTopology, SocketUpstreamTlsSettings,
    sort_protocol_document_rules,
};
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolDirection, ProtocolPackageKind, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketDownstreamTlsConfig, SocketRelaySecurity};

use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use super::{ListenerRuntimeAdapter, ProtocolDocumentRuleConnectionFactory};

/// 一次 Listener start 冻结的脚本、拓扑、规则、证书引用与资源限制。
#[derive(Clone)]
pub(super) struct ScriptedSocketRuntimeSnapshot {
    package: RuntimeProtocolPackageSnapshot,
    topology: SocketTopology,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
    document_rules: ProtocolDocumentRuleConnectionFactory,
    rule_generation: Arc<AtomicU64>,
    certificate_references: Arc<[CertificateReference]>,
    security: ScriptedSocketSecuritySnapshot,
    maximum_connections: u16,
    connect_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
}

/// 已解析的传输安全材料；LocalResponder 结构上没有任何 upstream TLS/connector 字段。
#[derive(Clone)]
pub(super) enum ScriptedSocketSecuritySnapshot {
    Relay(SocketRelaySecurity),
    LocalResponder {
        downstream_tls: Option<SocketDownstreamTlsConfig>,
    },
}

impl ScriptedSocketRuntimeSnapshot {
    pub(super) async fn prepare_async(
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        security: ScriptedSocketSecuritySnapshot,
    ) -> AppResult<Option<Arc<Self>>> {
        let intercept_proxy_domain::ListenerDataPlane::Socket(socket) = &listener.data_plane else {
            return Ok(None);
        };
        let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
            return Ok(None);
        };
        let package = adapter
            .protocol_packages
            .freeze_for_listener_start_async(&scripted.package)
            .await?;
        if package.compiled().kind() != ProtocolPackageKind::Socket {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_KIND_MISMATCH",
                "Socket 报文处理必须绑定 Socket 协议包。",
            )
            .entity(listener.id.to_string()));
        }
        let upstream = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
        let downstream = DirectionExecutionPlan::new(ProtocolDirection::Downstream);

        let workspace_for_compile = workspace.clone();
        let listener_for_compile = listener.clone();
        let package_ref = scripted.package.clone();
        let upstream_schema = package
            .compiled()
            .schema(ProtocolDirection::Upstream)
            .clone();
        let downstream_schema = package
            .compiled()
            .schema(ProtocolDirection::Downstream)
            .clone();
        let topology = socket.topology.clone();
        let document_rules = adapter
            .compile_document_rules_on_blocking_owner(move || {
                compile_document_rules(
                    &workspace_for_compile,
                    &listener_for_compile,
                    &package_ref,
                    &upstream_schema,
                    &downstream_schema,
                    &topology,
                )
            })
            .await?;
        Ok(Some(Arc::new(Self {
            package,
            topology: socket.topology.clone(),
            upstream,
            downstream,
            document_rules,
            rule_generation: Arc::new(AtomicU64::new(0)),
            certificate_references: selected_certificate_references(workspace, &socket.topology),
            security,
            maximum_connections: socket.maximum_connections,
            connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
            read_timeout: Duration::from_millis(listener.read_timeout_ms),
            write_timeout: Duration::from_millis(listener.write_timeout_ms),
        })))
    }

    pub(super) const fn topology(&self) -> &SocketTopology {
        &self.topology
    }

    pub(super) fn package(&self) -> &RuntimeProtocolPackageSnapshot {
        &self.package
    }

    pub(super) const fn upstream(&self) -> DirectionExecutionPlan {
        self.upstream
    }

    pub(super) const fn downstream(&self) -> DirectionExecutionPlan {
        self.downstream
    }

    #[cfg(test)]
    pub(super) fn rules(&self) -> Vec<ProtocolDocumentRuleDefinition> {
        let mut rules = [
            ProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream,
            ProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp,
        ]
        .into_iter()
        .flat_map(|stage| self.document_rules.program(stage).rules().to_vec())
        .collect::<Vec<_>>();
        rules.sort_by(|left, right| {
            left.priority()
                .cmp(&right.priority())
                .then_with(|| left.created_order().cmp(&right.created_order()))
                .then_with(|| left.rule_id().cmp(&right.rule_id()))
        });
        rules
    }

    #[cfg(test)]
    pub(super) fn rule_program(
        &self,
        stage: ProtocolRuleStage,
    ) -> Arc<ProtocolDocumentRuleProgram> {
        self.document_rules.program(stage)
    }

    /// 返回启动时已编译并冻结的规则连接工厂。
    pub(super) const fn rule_connections(&self) -> &ProtocolDocumentRuleConnectionFactory {
        &self.document_rules
    }

    pub(super) async fn replace_document_rules(
        &self,
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<()> {
        let intercept_proxy_domain::ListenerDataPlane::Socket(socket) = &listener.data_plane else {
            return Ok(());
        };
        let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
            return Ok(());
        };
        let generation = self.rule_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let workspace = workspace.clone();
        let listener = listener.clone();
        let package = scripted.package.clone();
        let upstream_schema = self
            .package
            .compiled()
            .schema(ProtocolDirection::Upstream)
            .clone();
        let downstream_schema = self
            .package
            .compiled()
            .schema(ProtocolDirection::Downstream)
            .clone();
        let topology = socket.topology.clone();
        let replacement = adapter
            .compile_document_rules_on_blocking_owner(move || {
                compile_document_rules(
                    &workspace,
                    &listener,
                    &package,
                    &upstream_schema,
                    &downstream_schema,
                    &topology,
                )
            })
            .await?;
        if self.rule_generation.load(Ordering::Acquire) == generation {
            self.document_rules.replace(&replacement);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn certificate_references(&self) -> &[CertificateReference] {
        &self.certificate_references
    }

    pub(super) const fn security(&self) -> &ScriptedSocketSecuritySnapshot {
        &self.security
    }

    pub(super) const fn runtime_limits(&self) -> ProtocolRuntimeLimits {
        self.package.runtime_limits()
    }

    pub(super) const fn maximum_connections(&self) -> u16 {
        self.maximum_connections
    }
}

impl std::fmt::Debug for ScriptedSocketRuntimeSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptedSocketRuntimeSnapshot")
            .field("package", self.package.compiled().package())
            .field("topology", &self.topology)
            .field("upstream", &self.upstream)
            .field("downstream", &self.downstream)
            .field("document_rules", self.rule_connections())
            .field(
                "certificate_reference_count",
                &self.certificate_references.len(),
            )
            .field("security", &self.security)
            .field("maximum_connections", &self.maximum_connections)
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("write_timeout", &self.write_timeout)
            .field("runtime_limits", &self.package.runtime_limits())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ScriptedSocketSecuritySnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Relay(SocketRelaySecurity::Transparent) => {
                formatter.write_str("Relay(Transparent)")
            }
            Self::Relay(SocketRelaySecurity::TcpToTls { upstream_tls }) => formatter
                .debug_struct("Relay(TcpToTls)")
                .field("server_trust_count", &upstream_tls.server_trust_der.len())
                .field(
                    "has_client_identity",
                    &upstream_tls.client_identity.is_some(),
                )
                .field("verify_hostname", &upstream_tls.verify_hostname)
                .finish(),
            Self::Relay(SocketRelaySecurity::TlsToTcp { downstream_tls }) => formatter
                .debug_struct("Relay(TlsToTcp)")
                .field(
                    "server_certificate_count",
                    &downstream_tls.server_identity.certificate_chain_der.len(),
                )
                .field("client_trust_count", &downstream_tls.client_trust_der.len())
                .field(
                    "client_authentication_required",
                    &downstream_tls.client_authentication_required,
                )
                .finish(),
            Self::Relay(SocketRelaySecurity::TlsToTls {
                downstream_tls,
                upstream_tls,
            }) => formatter
                .debug_struct("Relay(TlsToTls)")
                .field(
                    "downstream_server_certificate_count",
                    &downstream_tls.server_identity.certificate_chain_der.len(),
                )
                .field(
                    "downstream_client_trust_count",
                    &downstream_tls.client_trust_der.len(),
                )
                .field(
                    "downstream_client_authentication_required",
                    &downstream_tls.client_authentication_required,
                )
                .field(
                    "upstream_server_trust_count",
                    &upstream_tls.server_trust_der.len(),
                )
                .field(
                    "upstream_has_client_identity",
                    &upstream_tls.client_identity.is_some(),
                )
                .field("upstream_verify_hostname", &upstream_tls.verify_hostname)
                .finish(),
            Self::LocalResponder { downstream_tls } => formatter
                .debug_struct("LocalResponder")
                .field("tls_enabled", &downstream_tls.is_some())
                .field(
                    "server_certificate_count",
                    &downstream_tls
                        .as_ref()
                        .map_or(0, |tls| tls.server_identity.certificate_chain_der.len()),
                )
                .field(
                    "client_trust_count",
                    &downstream_tls
                        .as_ref()
                        .map_or(0, |tls| tls.client_trust_der.len()),
                )
                .field(
                    "client_authentication_required",
                    &downstream_tls
                        .as_ref()
                        .is_some_and(|tls| tls.client_authentication_required),
                )
                .finish(),
        }
    }
}

fn compile_rule_program(
    listener: &ProxyListener,
    package: &intercept_proxy_domain::ProtocolPackageRef,
    schema: &intercept_proxy_domain::DocumentSchemaNode,
    stage: ProtocolRuleStage,
    rules: &[ProtocolDocumentRuleDefinition],
) -> AppResult<ProtocolDocumentRuleProgram> {
    let selected = rules
        .iter()
        .filter(|rule| rule.stage() == stage)
        .cloned()
        .collect::<Vec<_>>();
    ProtocolDocumentRuleProgram::new_for_stage(
        listener.id,
        package.clone(),
        schema.clone(),
        stage,
        selected,
    )
    .map_err(AppError::from)
}

pub(super) fn compile_document_rules(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &intercept_proxy_domain::ProtocolPackageRef,
    upstream_schema: &intercept_proxy_domain::DocumentSchemaNode,
    downstream_schema: &intercept_proxy_domain::DocumentSchemaNode,
    topology: &SocketTopology,
) -> AppResult<ProtocolDocumentRuleConnectionFactory> {
    let mut rules = workspace
        .document_runtime_rules()?
        .into_iter()
        .filter(|rule| rule.listener_id() == listener.id)
        .collect::<Vec<_>>();
    for rule in &rules {
        let schema = match rule.direction() {
            RuleDirection::Upstream => upstream_schema,
            RuleDirection::Downstream => downstream_schema,
        };
        if rule.package() != package {
            return Err(AppError::new(
                "PROTOCOL_RULE_RUNTIME_BINDING_MISMATCH",
                "协议报文规则与当前协议包或 Schema 不一致。",
            )
            .entity(rule.rule_id().to_string()));
        }
        rule.validate_against_schema(schema)?;
        validate_rule_direction(rule, topology)?;
    }
    sort_protocol_document_rules(&mut rules);
    let compile = |stage: ProtocolRuleStage| {
        let schema = match stage.direction() {
            RuleDirection::Upstream => upstream_schema,
            RuleDirection::Downstream => downstream_schema,
        };
        compile_rule_program(listener, package, schema, stage, &rules).map(Arc::new)
    };
    ProtocolDocumentRuleConnectionFactory::new(
        compile(ProtocolRuleStage::AppToProxy)?,
        compile(ProtocolRuleStage::ProxyToUpstream)?,
        compile(ProtocolRuleStage::UpstreamToProxy)?,
        compile(ProtocolRuleStage::ProxyToApp)?,
    )
    .map_err(AppError::from)
}

fn validate_rule_direction(
    rule: &ProtocolDocumentRuleDefinition,
    topology: &SocketTopology,
) -> AppResult<()> {
    match topology {
        SocketTopology::LocalResponder(_)
            if !matches!(
                rule.stage(),
                ProtocolRuleStage::AppToProxy | ProtocolRuleStage::ProxyToApp
            ) =>
        {
            Err(AppError::new(
                "PROTOCOL_RULE_DIRECTION_INVALID",
                "本机应答运行快照只接受“应用 → 代理”和“代理 → 应用”规则。",
            )
            .entity(rule.rule_id().to_string()))
        }
        SocketTopology::Relay(_) | SocketTopology::LocalResponder(_) => Ok(()),
    }
}

pub(super) fn selected_certificate_references(
    workspace: &ProxyWorkspace,
    topology: &SocketTopology,
) -> Arc<[CertificateReference]> {
    let mut selected = BTreeSet::new();
    match topology {
        SocketTopology::Relay(relay) => match &relay.security {
            DomainSecurity::Transparent => {}
            DomainSecurity::TcpToTls { upstream_tls } => {
                select_upstream_tls(&mut selected, upstream_tls);
            }
            DomainSecurity::TlsToTcp { downstream_tls } => {
                select_downstream_tls(&mut selected, downstream_tls);
            }
            DomainSecurity::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => {
                select_downstream_tls(&mut selected, downstream_tls);
                select_upstream_tls(&mut selected, upstream_tls);
            }
        },
        SocketTopology::LocalResponder(local) => {
            if let SocketDownstreamSecurity::Tls { downstream_tls } = &local.downstream_security {
                select_downstream_tls(&mut selected, downstream_tls);
            }
        }
    }
    workspace
        .certificate_references
        .iter()
        .filter(|reference| selected.contains(&reference.id))
        .cloned()
        .collect::<Vec<_>>()
        .into()
}

fn select_downstream_tls(
    selected: &mut BTreeSet<CertificateReferenceId>,
    settings: &SocketDownstreamTlsSettings,
) {
    selected.insert(settings.server_identity);
    match settings.client_authentication {
        DownstreamClientAuthentication::Disabled => {}
        DownstreamClientAuthentication::Optional { trust }
        | DownstreamClientAuthentication::Required { trust } => {
            selected.insert(trust);
        }
    }
}

fn select_upstream_tls(
    selected: &mut BTreeSet<CertificateReferenceId>,
    settings: &SocketUpstreamTlsSettings,
) {
    selected.extend(settings.server_trust);
    selected.extend(settings.client_identity);
}
