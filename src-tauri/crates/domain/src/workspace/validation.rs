//! Workspace 配置的聚合校验。

use std::collections::BTreeSet;

use super::{DomainError, ListenerId, ProxyWorkspace, ResponseAssertion, ResponseAssertionKind};

mod listener;
mod value;

pub(crate) use listener::validate_listener;
pub use value::{is_valid_cidr, is_valid_upstream_origin};

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

    validate_android_profiles(workspace, listener_ids, error);
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
