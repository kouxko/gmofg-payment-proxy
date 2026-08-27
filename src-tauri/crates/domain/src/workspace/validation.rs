//! Workspace 配置的聚合校验。

use std::collections::BTreeSet;

use super::{
    DomainError, HttpBodyProcessing, ListenerDataPlane, ListenerId, ProxyWorkspace,
    SocketPayloadProcessing, SocketTopology,
};
use crate::{
    MAX_JAVASCRIPT_SAFE_INTEGER, MAX_PROTOCOL_DOCUMENT_RULES, ProtocolDocumentRuleDefinition,
    ProtocolRuleStage,
};

mod listener;
mod value;

pub(crate) use listener::validate_listener;
pub use value::{is_valid_socket_host, is_valid_upstream_origin};

pub(super) fn validate_workspace_references(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    unique_ids(workspace.rules.iter().map(|item| item.id), "rules", error);
    for (index, rule) in workspace.rules.iter().enumerate() {
        if let Some(channel) = &rule.channel
            && !listener_ids
                .iter()
                .any(|listener_id| listener_id.to_string() == channel.as_str())
        {
            push_field_error(
                error,
                format!("rules.{index}.channel"),
                "规则通道必须引用当前 Workspace 中存在的代理入口",
            );
        }
    }

    validate_protocol_rules(workspace, listener_ids, error);

    validate_android_profiles(workspace, listener_ids, error);
}

fn validate_protocol_rules(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    if workspace.protocol_rule_created_order_high_water > MAX_JAVASCRIPT_SAFE_INTEGER {
        push_field_error(
            error,
            "protocol_rule_created_order_high_water",
            "协议报文规则创建顺序高水位不能超过 JavaScript 安全整数上限",
        );
    }
    if workspace
        .protocol_rules
        .iter()
        .map(ProtocolDocumentRuleDefinition::created_order)
        .max()
        .is_some_and(|maximum| maximum > workspace.protocol_rule_created_order_high_water)
    {
        push_field_error(
            error,
            "protocol_rule_created_order_high_water",
            "协议报文规则创建顺序高水位不能小于现存规则的 created_order",
        );
    }
    if workspace.protocol_rules.len() > MAX_PROTOCOL_DOCUMENT_RULES {
        push_field_error(
            error,
            "protocol_rules",
            "单个 Workspace 的 协议报文规则不能超过 1024 条",
        );
    }
    let mut rule_ids = BTreeSet::new();
    for (index, rule) in workspace.protocol_rules.iter().enumerate() {
        let prefix = format!("protocol_rules.{index}");
        if !rule_ids.insert(rule.rule_id()) {
            push_field_error(error, format!("{prefix}.rule_id"), "规则 ID 不能重复");
        }
        if !listener_ids.contains(&rule.listener_id()) {
            push_field_error(
                error,
                format!("{prefix}.listener_id"),
                "规则必须引用当前 Workspace 中存在的监听器",
            );
            continue;
        }
        let Some(listener) = workspace
            .listeners
            .iter()
            .find(|item| item.id == rule.listener_id())
        else {
            continue;
        };
        match &listener.data_plane {
            ListenerDataPlane::Http(settings) => match &settings.body_processing {
                HttpBodyProcessing::Plain => push_field_error(
                    error,
                    format!("{prefix}.listener_id"),
                    "报文规则只能绑定已选择协议方案的入口",
                ),
                HttpBodyProcessing::Protocol { package } if package != rule.package() => {
                    push_field_error(
                        error,
                        format!("{prefix}.package"),
                        "规则包版本必须与入口精确绑定一致",
                    );
                }
                HttpBodyProcessing::Protocol { .. } => {}
            },
            ListenerDataPlane::Socket(settings) => {
                let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
                    push_field_error(
                        error,
                        format!("{prefix}.listener_id"),
                        "报文规则只能绑定已选择协议方案的入口",
                    );
                    continue;
                };
                if &scripted.package != rule.package() {
                    push_field_error(
                        error,
                        format!("{prefix}.package"),
                        "规则包版本必须与入口精确绑定一致",
                    );
                }
                match &settings.topology {
                    SocketTopology::Relay(_) => {}
                    SocketTopology::LocalResponder(_) => {
                        if !matches!(
                            rule.stage(),
                            ProtocolRuleStage::AppToProxy | ProtocolRuleStage::ProxyToApp
                        ) {
                            push_field_error(
                                error,
                                format!("{prefix}.stage"),
                                "本机应答只允许配置应用进入代理和代理返回应用两个阶段",
                            );
                        }
                    }
                }
            }
        }
    }
}

fn validate_android_profiles(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    let mut profile_ids = BTreeSet::new();
    for (index, profile) in workspace.android_network_profiles.iter().enumerate() {
        if !profile_ids.insert(profile.id.as_str()) {
            push_field_error(
                error,
                format!("android_network_profiles.{index}.id"),
                "设备网络方案 ID 不能重复",
            );
        }
        if let Err(profile_error) = profile.validate() {
            for (field, messages) in profile_error.field_errors.iter() {
                for message in messages {
                    push_field_error(
                        error,
                        format!("android_network_profiles.{index}.{field}"),
                        message.clone(),
                    );
                }
            }
        }
        for (route_index, route) in profile.proxy_routes.iter().enumerate() {
            if !listener_ids.contains(&route.listener_id) {
                push_field_error(
                    error,
                    format!(
                        "android_network_profiles.{index}.proxy_routes.{route_index}.listener_id"
                    ),
                    "透明代理路由必须引用当前 Workspace 中存在的代理入口",
                );
            }
        }
    }
}

pub(super) fn unique_ids<T: Copy + Ord>(
    values: impl Iterator<Item = T>,
    field: &str,
    error: &mut DomainError,
) -> BTreeSet<T> {
    let mut ids = BTreeSet::new();
    for (index, id) in values.enumerate() {
        if !ids.insert(id) {
            push_field_error(error, format!("{field}.{index}.id"), "ID 不能重复");
        }
    }
    ids
}

pub(super) fn push_field_error(
    error: &mut DomainError,
    field: impl Into<String>,
    message: impl Into<String>,
) {
    error
        .field_errors
        .entry(field.into())
        .or_default()
        .push(message.into());
}
