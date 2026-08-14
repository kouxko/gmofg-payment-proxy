//! Scripted Socket Listener 的不可变启动快照。

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, DownstreamClientAuthentication, ProxyListener,
    ProxyWorkspace, SocketDirection, SocketDocumentRuleDefinition, SocketDownstreamSecurity,
    SocketDownstreamTlsSettings, SocketPayloadProcessing, SocketRelaySecurity as DomainSecurity,
    SocketTopology, SocketUpstreamTlsSettings, sort_socket_document_rules,
};
#[cfg(test)]
use intercept_proxy_protocol_scripting::ProtocolRuntimeLimits;
use intercept_proxy_protocol_scripting::{DirectionExecutionPlan, ProtocolDirection};
use intercept_proxy_runtime::{SocketDownstreamTlsConfig, SocketRelaySecurity};

use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use super::ListenerRuntimeAdapter;

/// 一次 Listener start 冻结的脚本、拓扑、规则、证书引用与资源限制。
#[derive(Clone)]
pub(super) struct ScriptedSocketRuntimeSnapshot {
    package: RuntimeProtocolPackageSnapshot,
    topology: SocketTopology,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
    rules: Arc<[SocketDocumentRuleDefinition]>,
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
        let repository = adapter.protocol_packages.as_ref().ok_or_else(|| {
            AppError::new(
                "PROTOCOL_PACKAGE_SERVICES_UNAVAILABLE",
                "Scripted Socket 运行时没有装配协议包注册表。",
            )
            .entity(listener.id.to_string())
        })?;
        let package = repository.freeze_for_listener_start(&scripted.package)?;
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

        let mut rules = workspace
            .socket_rules
            .iter()
            .filter(|rule| rule.listener_id() == listener.id)
            .cloned()
            .collect::<Vec<_>>();
        for rule in &rules {
            if rule.package() != &scripted.package
                || rule.schema_version() != package.compiled().schema().version()
            {
                return Err(AppError::new(
                    "SOCKET_RULE_RUNTIME_BINDING_MISMATCH",
                    "Socket 规则与启动时冻结的协议包或 Schema 不一致。",
                )
                .entity(rule.rule_id().to_string()));
            }
            rule.validate_against_schema(package.compiled().schema())?;
            validate_rule_direction(rule, &socket.topology, upstream, downstream)?;
        }
        sort_socket_document_rules(&mut rules);
        Ok(Some(Arc::new(Self {
            package,
            topology: socket.topology.clone(),
            upstream,
            downstream,
            rules: rules.into(),
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

    #[cfg(test)]
    pub(super) fn package(&self) -> &RuntimeProtocolPackageSnapshot {
        &self.package
    }

    #[cfg(test)]
    pub(super) const fn upstream(&self) -> DirectionExecutionPlan {
        self.upstream
    }

    #[cfg(test)]
    pub(super) const fn downstream(&self) -> DirectionExecutionPlan {
        self.downstream
    }

    #[cfg(test)]
    pub(super) fn rules(&self) -> &[SocketDocumentRuleDefinition] {
        &self.rules
    }

    #[cfg(test)]
    pub(super) fn certificate_references(&self) -> &[CertificateReference] {
        &self.certificate_references
    }

    #[cfg(test)]
    pub(super) const fn security(&self) -> &ScriptedSocketSecuritySnapshot {
        &self.security
    }

    #[cfg(test)]
    pub(super) const fn runtime_limits(&self) -> ProtocolRuntimeLimits {
        self.package.runtime_limits()
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
            .field("rule_count", &self.rules.len())
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
        SocketTopology::LocalResponder(_) if rule.direction() != SocketDirection::Downstream => {
            Err(AppError::new(
                "SOCKET_RULE_DIRECTION_INVALID",
                "LocalResponder 运行快照只接受 downstream 响应规则。",
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
