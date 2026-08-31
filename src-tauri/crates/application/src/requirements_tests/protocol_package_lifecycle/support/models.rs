use super::*;

pub(in super::super) fn package(id: &str, version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

pub(in super::super) fn record(
    package: ProtocolPackageRef,
    enabled: bool,
) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        name: format!("{} {}", package.id, package.version),
        package,
        host_api: 1,
        kind: ProtocolPackageKindViewModel::Socket,
        source: ProtocolPackageSourceViewModel::External { online: true },
        enabled,
        validation: ProtocolPackageValidationViewModel::Valid,
        installed_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
    }
}

pub(in super::super) fn usage(
    workspace_id: WorkspaceId,
    listener_id: ListenerId,
    runtime_state: ListenerRuntimeState,
) -> ProtocolPackageUsageViewModel {
    ProtocolPackageUsageViewModel {
        workspace_id,
        workspace_name: format!("Workspace {workspace_id}"),
        listener_id,
        listener_name: format!("Listener {listener_id}"),
        listener_enabled: runtime_state != ListenerRuntimeState::Stopped,
        runtime_state,
    }
}

pub(in super::super) fn description(
    package: ProtocolPackageRef,
) -> ProtocolPackageDescriptionViewModel {
    let upstream_schema = ProtocolPackageSchemaViewModel {
        root: intercept_proxy_domain::DocumentSchemaNode::Object {
            title: Some("Payments".into()),
            properties: std::collections::BTreeMap::from([
                (
                    "trace_id".into(),
                    intercept_proxy_domain::DocumentSchemaNode::String {
                        title: Some("Trace ID".into()),
                    },
                ),
                (
                    "amount".into(),
                    intercept_proxy_domain::DocumentSchemaNode::Number {
                        title: Some("Amount".into()),
                    },
                ),
                (
                    "approved".into(),
                    intercept_proxy_domain::DocumentSchemaNode::Boolean {
                        title: Some("Approved".into()),
                    },
                ),
            ]),
        },
    };
    ProtocolPackageDescriptionViewModel {
        package,
        kind: ProtocolPackageKindViewModel::Socket,
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            downstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: false,
            },
            display: true,
        },
        upstream_schema: Some(upstream_schema.clone()),
        downstream_schema: Some(ProtocolPackageSchemaViewModel {
            root: upstream_schema.root,
        }),
    }
}

pub(in super::super) fn strict_description(
    package: ProtocolPackageRef,
) -> ProtocolPackageDescriptionViewModel {
    let mut value = description(package);
    value.capabilities.downstream.encode = true;
    value
}
