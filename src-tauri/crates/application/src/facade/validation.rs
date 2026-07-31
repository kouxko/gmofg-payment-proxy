//! 应用用例共用的规范化和校验辅助函数。
//!
//! 这些函数返回稳定字段错误和规范化值，任何展示适配器都不应复制这部分业务逻辑。

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AppError, AppResult, FieldValidationViewModel, SettingsDraft, SettingsValidationViewModel,
};

pub(super) fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(super) fn normalize_sans(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub(super) fn normalize_certificate_sans(values: &[String]) -> Vec<String> {
    normalize_sans(
        values
            .iter()
            .map(|value| {
                let value = value.trim();
                value
                    .split_once(':')
                    .filter(|(kind, _)| {
                        kind.eq_ignore_ascii_case("DNS") || kind.eq_ignore_ascii_case("IP")
                    })
                    .map_or(value, |(_, address)| address)
                    .to_owned()
            })
            .collect(),
    )
}

pub(super) fn parse_sans_raw(raw: &str) -> Vec<String> {
    normalize_sans(raw.split([',', '，']).map(ToOwned::to_owned).collect())
}

pub(super) fn normalize_settings(mut draft: SettingsDraft) -> SettingsDraft {
    draft.bind_address = draft.bind_address.trim().to_owned();
    for channel in &mut draft.channels {
        channel.display_name = channel.display_name.trim().to_owned();
        channel.upstream_url = channel.upstream_url.trim().to_owned();
    }
    draft.leaf_sans = normalize_sans(draft.leaf_sans);
    draft
}

pub(super) fn validate_settings_locally(draft: &SettingsDraft) -> SettingsValidationViewModel {
    let mut field_errors = BTreeMap::new();
    if !draft.channels.iter().any(|channel| channel.enabled) {
        push_error(&mut field_errors, "channels", "至少启用一个代理通道。");
    }
    let mut ids = BTreeSet::new();
    let mut ports = BTreeMap::new();
    for channel in &draft.channels {
        let prefix = format!("channels.{}", channel.id.as_str());
        if !ids.insert(channel.id.as_str()) {
            push_error(
                &mut field_errors,
                &format!("{prefix}.id"),
                "通道 ID 不能重复。",
            );
        }
        if channel.display_name.is_empty() {
            push_error(
                &mut field_errors,
                &format!("{prefix}.display_name"),
                "通道显示名不能为空。",
            );
        }
        if channel.enabled {
            if let Some(existing) = ports.insert(channel.port, channel.id.as_str()) {
                push_error(
                    &mut field_errors,
                    &format!("{prefix}.port"),
                    &format!("监听端口与通道 {existing} 重复。"),
                );
            }
            if !gmofg_proxy_domain::is_valid_https_upstream_url(&channel.upstream_url) {
                push_error(
                    &mut field_errors,
                    &format!("{prefix}.upstream_url"),
                    "上游 URL 必须是 HTTPS origin（仅主机和可选端口）。",
                );
            }
        }
    }
    if draft.bind_address.is_empty() {
        push_error(&mut field_errors, "bind_address", "绑定地址不能为空。");
    }
    for (field, timeout) in [
        ("connect_timeout_seconds", draft.connect_timeout_seconds),
        ("write_timeout_seconds", draft.write_timeout_seconds),
        ("read_timeout_seconds", draft.read_timeout_seconds),
    ] {
        if timeout == 0 || timeout > 600 {
            push_error(&mut field_errors, field, "超时必须位于 1 到 600 秒之间。");
        }
    }
    if draft.max_body_bytes == 0 || draft.max_body_bytes > 64 * 1024 * 1024 {
        push_error(
            &mut field_errors,
            "max_body_bytes",
            "单个 Body 上限必须位于 1 字节到 64 MiB 之间。",
        );
    }
    if draft.max_sessions == 0 {
        push_error(&mut field_errors, "max_sessions", "会话容量必须至少为 1。");
    }
    if draft.max_memory_bytes == 0 {
        push_error(
            &mut field_errors,
            "max_memory_bytes",
            "内存容量必须至少为 1 字节。",
        );
    }
    SettingsValidationViewModel {
        valid: field_errors.is_empty(),
        field_errors,
        warnings: Vec::new(),
    }
}

pub(super) fn push_error(errors: &mut BTreeMap<String, Vec<String>>, field: &str, message: &str) {
    errors
        .entry(field.to_owned())
        .or_default()
        .push(message.to_owned());
}

pub(super) fn ensure_valid(
    code: &str,
    message: &str,
    validation: &FieldValidationViewModel,
) -> AppResult<()> {
    if validation.valid {
        Ok(())
    } else {
        Err(AppError::field(
            code,
            message,
            validation.field_errors.clone(),
        ))
    }
}

pub(super) fn require_confirmation(confirmed: bool, message: &str) -> AppResult<()> {
    if confirmed {
        Ok(())
    } else {
        Err(AppError::new("CONFIRMATION_REQUIRED", message))
    }
}
