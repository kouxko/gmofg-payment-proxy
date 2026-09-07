use intercept_proxy_domain::{
    FixedServerSettings, HttpListenerSettings, HttpTopology, ListenerDataPlane, SocketEndpoint,
    SocketRelaySettings, SocketUpstreamTlsSettings, UpstreamTlsSettings,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    DownstreamClientAuthenticationDraft, ListenerDataPlaneDraft, ListenerDraft,
    ScriptedSocketProcessingDraft, SocketDownstreamSecurityDraft, SocketDownstreamTlsDraftSettings,
    SocketPayloadProcessingDraft, SocketRelayDraftSettings, SocketRelaySecurityDraft,
    SocketRelayTopologyDraft, SocketTopologyDraft,
};
use crate::{AppError, AppResult, ListenerDataPlaneKind, ProtocolPackageRef};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpResponseMode {
    RequestTarget,
    FixedServer,
    LocalServer,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketTopologyMode {
    Relay,
    LocalResponder,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketProcessingMode {
    Direct,
    Scripted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenerDraftTransition {
    DataPlane { kind: ListenerDataPlaneKind },
    HttpResponseMode { mode: HttpResponseMode },
    SocketTopology { mode: SocketTopologyMode },
    Processing { mode: SocketProcessingMode },
    AppTls { enabled: bool },
    ServerTls { enabled: bool },
}

impl ListenerDraft {
    pub(crate) fn transition(
        mut self,
        transition: ListenerDraftTransition,
        recommended_package: Option<ProtocolPackageRef>,
    ) -> AppResult<Self> {
        match transition {
            ListenerDraftTransition::DataPlane { kind } => {
                let current = match self.data_plane {
                    ListenerDataPlaneDraft::Http(_) => ListenerDataPlaneKind::Http,
                    ListenerDataPlaneDraft::Socket(_) => ListenerDataPlaneKind::Socket,
                };
                if current != kind {
                    self.data_plane = match kind {
                        ListenerDataPlaneKind::Http => {
                            ListenerDataPlane::Http(HttpListenerSettings::default()).into()
                        }
                        ListenerDataPlaneKind::Socket => {
                            ListenerDataPlane::Socket(SocketRelaySettings::default()).into()
                        }
                    };
                }
            }
            ListenerDraftTransition::HttpResponseMode { mode } => {
                let ListenerDataPlaneDraft::Http(settings) = &mut self.data_plane else {
                    return Err(not_applicable("http_response_mode", "socket"));
                };
                let topology = match mode {
                    HttpResponseMode::RequestTarget => HttpTopology::remote(None),
                    HttpResponseMode::FixedServer => {
                        HttpTopology::remote(Some(FixedServerSettings {
                            upstream_url: String::new(),
                            upstream_tls: UpstreamTlsSettings::default(),
                        }))
                    }
                    HttpResponseMode::LocalServer => HttpTopology::LocalServer,
                };
                if response_mode(&settings.topology) != mode {
                    settings.topology = topology;
                }
            }
            ListenerDraftTransition::SocketTopology { mode } => {
                let ListenerDataPlaneDraft::Socket(settings) = &mut self.data_plane else {
                    return Err(not_applicable("socket_topology", "http"));
                };
                set_socket_topology(settings, mode, recommended_package);
            }
            ListenerDraftTransition::Processing { mode } => {
                let ListenerDataPlaneDraft::Socket(settings) = &mut self.data_plane else {
                    return Err(not_applicable("processing", "http"));
                };
                set_processing(settings, mode, recommended_package);
            }
            ListenerDraftTransition::AppTls { enabled } => {
                let ListenerDataPlaneDraft::Socket(settings) = &mut self.data_plane else {
                    return Err(not_applicable("app_tls", "http"));
                };
                set_app_tls(settings, enabled);
            }
            ListenerDraftTransition::ServerTls { enabled } => {
                let ListenerDataPlaneDraft::Socket(settings) = &mut self.data_plane else {
                    return Err(not_applicable("server_tls", "http"));
                };
                let SocketTopologyDraft::Relay(relay) = &mut settings.topology else {
                    return Err(not_applicable("server_tls", "local_responder"));
                };
                set_server_tls(relay, enabled);
            }
        }
        Ok(self)
    }
}

fn not_applicable(transition: &str, data_plane: &str) -> AppError {
    AppError::new(
        "LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE",
        format!("{transition} 转换不适用于 {data_plane} Listener。"),
    )
}

fn response_mode(topology: &HttpTopology) -> HttpResponseMode {
    match topology {
        HttpTopology::LocalServer => HttpResponseMode::LocalServer,
        HttpTopology::RemoteServer(remote) if remote.fixed_server.is_some() => {
            HttpResponseMode::FixedServer
        }
        HttpTopology::RemoteServer(_) => HttpResponseMode::RequestTarget,
    }
}

fn set_socket_topology(
    settings: &mut SocketRelayDraftSettings,
    mode: SocketTopologyMode,
    recommended_package: Option<ProtocolPackageRef>,
) {
    if matches!(
        (&settings.topology, mode),
        (SocketTopologyDraft::Relay(_), SocketTopologyMode::Relay)
            | (
                SocketTopologyDraft::LocalResponder(_),
                SocketTopologyMode::LocalResponder
            )
    ) {
        return;
    }
    let app_tls = take_app_tls(&settings.topology);
    settings.topology = match mode {
        SocketTopologyMode::Relay => SocketTopologyDraft::Relay(SocketRelayTopologyDraft {
            upstream: SocketEndpoint::default(),
            security: relay_security(app_tls, None),
        }),
        SocketTopologyMode::LocalResponder => {
            if matches!(settings.processing, SocketPayloadProcessingDraft::Direct) {
                settings.processing = scripted(recommended_package);
            }
            SocketTopologyDraft::LocalResponder(super::SocketLocalResponderTopologyDraft {
                downstream_security: app_tls
                    .map_or(SocketDownstreamSecurityDraft::Tcp, |downstream_tls| {
                        SocketDownstreamSecurityDraft::Tls { downstream_tls }
                    }),
            })
        }
    };
}

fn set_processing(
    settings: &mut SocketRelayDraftSettings,
    mode: SocketProcessingMode,
    recommended_package: Option<ProtocolPackageRef>,
) {
    match mode {
        SocketProcessingMode::Direct => {
            if matches!(settings.topology, SocketTopologyDraft::LocalResponder(_)) {
                set_socket_topology(settings, SocketTopologyMode::Relay, None);
            }
            if matches!(settings.processing, SocketPayloadProcessingDraft::Direct) {
                return;
            }
            settings.processing = SocketPayloadProcessingDraft::Direct;
        }
        SocketProcessingMode::Scripted => {
            if !matches!(
                settings.processing,
                SocketPayloadProcessingDraft::Scripted(_)
            ) {
                settings.processing = scripted(recommended_package);
            }
        }
    }
}

fn set_app_tls(settings: &mut SocketRelayDraftSettings, enabled: bool) {
    let existing = take_app_tls(&settings.topology);
    let app_tls = if enabled {
        Some(existing.unwrap_or(SocketDownstreamTlsDraftSettings {
            server_identity: None,
            client_authentication: DownstreamClientAuthenticationDraft::Disabled,
        }))
    } else {
        None
    };
    match &mut settings.topology {
        SocketTopologyDraft::Relay(relay) => {
            let upstream_tls = take_server_tls(&relay.security);
            relay.security = relay_security(app_tls, upstream_tls);
        }
        SocketTopologyDraft::LocalResponder(local) => {
            local.downstream_security = app_tls
                .map_or(SocketDownstreamSecurityDraft::Tcp, |downstream_tls| {
                    SocketDownstreamSecurityDraft::Tls { downstream_tls }
                });
        }
    }
}

fn set_server_tls(relay: &mut SocketRelayTopologyDraft, enabled: bool) {
    let downstream_tls = take_app_tls_from_relay(&relay.security);
    let existing = take_server_tls(&relay.security);
    let upstream_tls = if enabled {
        Some(existing.unwrap_or_default())
    } else {
        None
    };
    relay.security = relay_security(downstream_tls, upstream_tls);
}

fn scripted(package: Option<ProtocolPackageRef>) -> SocketPayloadProcessingDraft {
    SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft { package })
}

fn take_app_tls(topology: &SocketTopologyDraft) -> Option<SocketDownstreamTlsDraftSettings> {
    match topology {
        SocketTopologyDraft::Relay(relay) => take_app_tls_from_relay(&relay.security),
        SocketTopologyDraft::LocalResponder(local) => match &local.downstream_security {
            SocketDownstreamSecurityDraft::Tcp => None,
            SocketDownstreamSecurityDraft::Tls { downstream_tls } => Some(downstream_tls.clone()),
        },
    }
}

fn take_app_tls_from_relay(
    security: &SocketRelaySecurityDraft,
) -> Option<SocketDownstreamTlsDraftSettings> {
    match security {
        SocketRelaySecurityDraft::TlsToTcp { downstream_tls }
        | SocketRelaySecurityDraft::TlsToTls { downstream_tls, .. } => Some(downstream_tls.clone()),
        SocketRelaySecurityDraft::Transparent | SocketRelaySecurityDraft::TcpToTls { .. } => None,
    }
}

fn take_server_tls(security: &SocketRelaySecurityDraft) -> Option<SocketUpstreamTlsSettings> {
    match security {
        SocketRelaySecurityDraft::TcpToTls { upstream_tls }
        | SocketRelaySecurityDraft::TlsToTls { upstream_tls, .. } => Some(upstream_tls.clone()),
        SocketRelaySecurityDraft::Transparent | SocketRelaySecurityDraft::TlsToTcp { .. } => None,
    }
}

fn relay_security(
    downstream_tls: Option<SocketDownstreamTlsDraftSettings>,
    upstream_tls: Option<SocketUpstreamTlsSettings>,
) -> SocketRelaySecurityDraft {
    match (downstream_tls, upstream_tls) {
        (None, None) => SocketRelaySecurityDraft::Transparent,
        (None, Some(upstream_tls)) => SocketRelaySecurityDraft::TcpToTls { upstream_tls },
        (Some(downstream_tls), None) => SocketRelaySecurityDraft::TlsToTcp { downstream_tls },
        (Some(downstream_tls), Some(upstream_tls)) => SocketRelaySecurityDraft::TlsToTls {
            downstream_tls,
            upstream_tls,
        },
    }
}
