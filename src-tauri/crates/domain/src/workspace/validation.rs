//! Workspace 配置的聚合校验。

use std::collections::BTreeSet;

use super::{
    DirectionProcessingOptions, DomainError, ListenerDataPlane, ListenerId, ProxyWorkspace,
    ResponseAssertion, ResponseAssertionKind, SocketPayloadProcessing, SocketTopology,
};
use crate::{
    MAX_JAVASCRIPT_SAFE_INTEGER, MAX_SOCKET_DOCUMENT_RULES, SocketDirection,
    SocketDocumentRuleDefinition,
};

mod listener;
mod value;

pub(crate) use listener::validate_listener;
pub use value::{is_valid_cidr, is_valid_socket_host, is_valid_upstream_origin};

pub(super) fn validate_workspace_references(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    unique_ids(
        workspace.metadata_extractors.iter().map(|item| item.id),
        "metadata_extractors",
        error,
    );
    for (index, extractor) in workspace.metadata_extractors.iter().enumerate() {
        validate_named_listener_refs(
            &extractor.name,
            &extractor.listener_ids,
            listener_ids,
            &format!("metadata_extractors.{index}"),
            error,
        );
    }

    unique_ids(
        workspace.response_assertions.iter().map(|item| item.id),
        "response_assertions",
        error,
    );
    for (index, assertion) in workspace.response_assertions.iter().enumerate() {
        validate_named_listener_refs(
            &assertion.name,
            &assertion.listener_ids,
            listener_ids,
            &format!("response_assertions.{index}"),
            error,
        );
        validate_assertion(assertion, index, error);
    }

    unique_ids(
        workspace.fault_presets.iter().map(|item| item.id),
        "fault_presets",
        error,
    );
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

    validate_socket_rules(workspace, listener_ids, error);

    validate_android_profiles(workspace, listener_ids, error);
}

fn validate_socket_rules(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    if workspace.socket_rule_created_order_high_water > MAX_JAVASCRIPT_SAFE_INTEGER {
        push_field_error(
            error,
            "socket_rule_created_order_high_water",
            "Socket 规则创建顺序高水位不能超过 JavaScript 安全整数上限",
        );
    }
    if workspace
        .socket_rules
        .iter()
        .map(SocketDocumentRuleDefinition::created_order)
        .max()
        .is_some_and(|maximum| maximum > workspace.socket_rule_created_order_high_water)
    {
        push_field_error(
            error,
            "socket_rule_created_order_high_water",
            "Socket 规则创建顺序高水位不能小于现存规则的 created_order",
        );
    }
    if workspace.socket_rules.len() > MAX_SOCKET_DOCUMENT_RULES {
        push_field_error(
            error,
            "socket_rules",
            "单个 Workspace 的 Socket 规则不能超过 1024 条",
        );
    }
    let mut rule_ids = BTreeSet::new();
    for (index, rule) in workspace.socket_rules.iter().enumerate() {
        let prefix = format!("socket_rules.{index}");
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
        let ListenerDataPlane::Socket(settings) = &listener.data_plane else {
            push_field_error(
                error,
                format!("{prefix}.listener_id"),
                "Socket 规则不能绑定 HTTP 监听器",
            );
            continue;
        };
        let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
            push_field_error(
                error,
                format!("{prefix}.listener_id"),
                "Socket 规则只能绑定 Scripted 监听器",
            );
            continue;
        };
        if &scripted.package != rule.package() {
            push_field_error(
                error,
                format!("{prefix}.package"),
                "规则包版本必须与监听器精确绑定一致",
            );
        }
        match &settings.topology {
            SocketTopology::Relay(_) => {
                let options = match rule.direction() {
                    SocketDirection::Upstream => scripted.upstream,
                    SocketDirection::Downstream => scripted.downstream,
                };
                validate_rule_options(rule.modifies_document(), options, &prefix, error);
            }
            SocketTopology::LocalResponder(_) => {
                if rule.direction() != SocketDirection::Downstream {
                    push_field_error(
                        error,
                        format!("{prefix}.direction"),
                        "LocalResponder 只允许 downstream 响应规则",
                    );
                }
                if rule.modifies_document() && !scripted.downstream.encode_enabled {
                    push_field_error(
                        error,
                        format!("{prefix}.actions"),
                        "修改 LocalResponder 响应需要开启 downstream Encode",
                    );
                }
            }
        }
    }
}

fn validate_rule_options(
    modifies_document: bool,
    options: DirectionProcessingOptions,
    prefix: &str,
    error: &mut DomainError,
) {
    if !options.decode_enabled {
        push_field_error(
            error,
            format!("{prefix}.direction"),
            "Relay 规则要求对应方向开启 Decode",
        );
    }
    if modifies_document && !options.encode_enabled {
        push_field_error(
            error,
            format!("{prefix}.actions"),
            "修改 Document 需要对应方向开启 Encode",
        );
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

fn validate_named_listener_refs(
    name: &str,
    listener_ids: &[ListenerId],
    existing: &BTreeSet<ListenerId>,
    prefix: &str,
    error: &mut DomainError,
) {
    if name.trim().is_empty() {
        push_field_error(error, format!("{prefix}.name"), "名称不能为空");
    }
    for (index, id) in listener_ids.iter().enumerate() {
        if !existing.contains(id) {
            push_field_error(
                error,
                format!("{prefix}.listener_ids.{index}"),
                "引用的监听器不存在",
            );
        }
    }
}

fn validate_assertion(assertion: &ResponseAssertion, index: usize, error: &mut DomainError) {
    let prefix = format!("response_assertions.{index}.assertion");
    match &assertion.assertion {
        ResponseAssertionKind::HttpStatusEquals { expected } if !(100..=599).contains(expected) => {
            push_field_error(error, prefix, "HTTP 状态码必须在 100..=599");
        }
        ResponseAssertionKind::HeaderEquals { name, .. } if name.trim().is_empty() => {
            push_field_error(error, prefix, "Header 名称不能为空");
        }
        ResponseAssertionKind::JsonPathEquals { path, .. } if path.trim().is_empty() => {
            push_field_error(error, prefix, "JSONPath 不能为空");
        }
        ResponseAssertionKind::BodySha256Equals { expected_hex }
            if expected_hex.len() != 64
                || !expected_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            push_field_error(error, prefix, "SHA-256 必须是 64 位十六进制字符串");
        }
        _ => {}
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
