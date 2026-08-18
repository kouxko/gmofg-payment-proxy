//! Scripted Socket Listener 的不可变启动快照。

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, DownstreamClientAuthentication, ProxyListener,
    ProxyWorkspace, SocketDirection, SocketDocumentRuleDefinition, SocketDocumentRuleProgram,
    SocketDownstreamSecurity, SocketDownstreamTlsSettings, SocketPayloadProcessing,
    SocketRelaySecurity as DomainSecurity, SocketRuleStage, SocketTopology,
    SocketUpstreamTlsSettings, sort_socket_document_rules,
};
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolDirection, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketDownstreamTlsConfig, SocketRelaySecurity};

use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use super::{ListenerRuntimeAdapter, SocketDocumentRuleConnectionFactory};

/// 一次 Listener start 冻结的脚本、拓扑、规则、证书引用与资源限制。
#[derive(Clone)]
pub(super) struct ScriptedSocketRuntimeSnapshot {
    package: RuntimeProtocolPackageSnapshot,
    topology: SocketTopology,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
    document_rules: SocketDocumentRuleConnectionFactory,
    certificate_references: Arc<[CertificateReference]>,
    security: ScriptedSocketSecuritySnapshot,
    allowed_client_cidrs: Arc<[String]>,
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
    pub(super) fn prepare(
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
            .freeze_for_listener_start(&scripted.package)?;
        let upstream = DirectionExecutionPlan::new(
            package.compiled(),
            ProtocolDirection::Upstream,
            scripted.upstream,
        )
        .map_err(|error| runtime_plan_error(listener, &error))?;
        let downstream = DirectionExecutionPlan::new(
            package.compiled(),
            ProtocolDirection::Downstream,
            scripted.downstream,
        )
        .map_err(|error| runtime_plan_error(listener, &error))?;

        let document_rules = compile_document_rules(
            workspace,
            listener,
            &scripted.package,
            package.compiled().schema(),
            &socket.topology,
            upstream,
            downstream,
        )?;
        Ok(Some(Arc::new(Self {
            package,
            topology: socket.topology.clone(),
            upstream,
            downstream,
            document_rules,
            certificate_references: selected_certificate_references(workspace, &socket.topology),
            security,
            allowed_client_cidrs: listener.allowed_client_cidrs.clone().into(),
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
    pub(super) fn rules(&self) -> Vec<SocketDocumentRuleDefinition> {
        let upstream = self.document_rules.program(SocketDirection::Upstream);
        let downstream = self.document_rules.program(SocketDirection::Downstream);
        let mut rules = upstream
            .rules()
            .iter()
            .chain(downstream.rules())
            .cloned()
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
        direction: SocketDirection,
    ) -> Arc<SocketDocumentRuleProgram> {
        self.document_rules.program(direction)
    }

    /// 返回启动时已编译并冻结的规则连接工厂。
    pub(super) const fn rule_connections(&self) -> &SocketDocumentRuleConnectionFactory {
        &self.document_rules
    }

    pub(super) fn replace_document_rules(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<()> {
        let intercept_proxy_domain::ListenerDataPlane::Socket(socket) = &listener.data_plane else {
            return Ok(());
        };
        let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
            return Ok(());
        };
        let replacement = compile_document_rules(
            workspace,
            listener,
            &scripted.package,
            self.package.compiled().schema(),
            &socket.topology,
            self.upstream,
            self.downstream,
        )?;
        self.document_rules.replace(replacement);
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
            .field(
                "allowed_client_cidr_count",
                &self.allowed_client_cidrs.len(),
            )
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
    schema: &intercept_proxy_domain::DocumentSchema,
    stage: SocketRuleStage,
    rules: &[SocketDocumentRuleDefinition],
) -> AppResult<SocketDocumentRuleProgram> {
    let selected = rules
        .iter()
        .filter(|rule| rule.stage() == stage)
        .cloned()
        .collect::<Vec<_>>();
    SocketDocumentRuleProgram::new_for_stage(
        listener.id,
        package.clone(),
        schema.clone(),
        stage,
        selected,
    )
    .map_err(AppError::from)
}

fn compile_document_rules(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &intercept_proxy_domain::ProtocolPackageRef,
    schema: &intercept_proxy_domain::DocumentSchema,
    topology: &SocketTopology,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
) -> AppResult<SocketDocumentRuleConnectionFactory> {
    let mut rules = workspace
        .socket_rules
        .iter()
        .filter(|rule| rule.listener_id() == listener.id)
        .cloned()
        .collect::<Vec<_>>();
    for rule in &rules {
        if rule.package() != package || rule.schema_version() != schema.version() {
            return Err(AppError::new(
                "SOCKET_RULE_RUNTIME_BINDING_MISMATCH",
                "Socket 规则与当前协议包或 Schema 不一致。",
            )
            .entity(rule.rule_id().to_string()));
        }
        rule.validate_against_schema(schema)?;
        validate_rule_direction(rule, topology, upstream, downstream)?;
    }
    sort_socket_document_rules(&mut rules);
    let compile =
        |stage| compile_rule_program(listener, package, schema, stage, &rules).map(Arc::new);
    SocketDocumentRuleConnectionFactory::new(
        compile(SocketRuleStage::AppToProxy)?,
        compile(SocketRuleStage::ProxyToUpstream)?,
        compile(SocketRuleStage::UpstreamToProxy)?,
        compile(SocketRuleStage::ProxyToApp)?,
    )
    .map_err(AppError::from)
}

fn validate_rule_direction(
    rule: &SocketDocumentRuleDefinition,
    topology: &SocketTopology,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
) -> AppResult<()> {
    let plan = match rule.direction() {
        SocketDirection::Upstream => upstream,
        SocketDirection::Downstream => downstream,
    };
    match topology {
        SocketTopology::Relay(_) if !plan.decode_enabled() => Err(AppError::new(
            "SOCKET_RULE_DECODE_REQUIRED",
            "Relay 规则要求对应方向在运行快照中开启 Decode。",
        )
        .entity(rule.rule_id().to_string())),
        SocketTopology::LocalResponder(_)
            if !matches!(
                rule.stage(),
                SocketRuleStage::AppToProxy | SocketRuleStage::ProxyToApp
            ) =>
        {
            Err(AppError::new(
                "SOCKET_RULE_DIRECTION_INVALID",
                "本机应答运行快照只接受“应用 → 代理”和“代理 → 应用”规则。",
            )
            .entity(rule.rule_id().to_string()))
        }
        _ if rule.modifies_document() && !plan.encode_enabled() => Err(AppError::new(
            "SOCKET_RULE_ENCODE_REQUIRED",
            "修改 Document 的规则要求运行快照开启 Encode。",
        )
        .entity(rule.rule_id().to_string())),
        _ => Ok(()),
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

fn runtime_plan_error(
    listener: &ProxyListener,
    error: &intercept_proxy_protocol_scripting::ProtocolRuntimeError,
) -> AppError {
    AppError::new(error.code(), "协议包入口能力不能满足 Listener 运行计划。")
        .entity(listener.id.to_string())
}
