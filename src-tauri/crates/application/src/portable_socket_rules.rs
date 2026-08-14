//! Socket 规则在旧版可移植文档中的兼容边界。
//!
//! 本地持久化可以保存规则，但 v3 可移植格式没有定义这些字段。空状态导出时移除内部
//! 字段，含规则状态时则明确拒绝，避免接收方静默丢失规则或高水位。

use serde_json::Value;

use crate::{AppError, AppResult, ProxyWorkspace};

const UNSUPPORTED_MESSAGE: &str =
    "当前 v3 可移植格式不支持 Socket 规则，请使用支持 Socket 规则的新版可移植格式。";

pub(crate) fn ensure_not_portable(workspaces: &[ProxyWorkspace]) -> AppResult<()> {
    if workspaces.iter().all(|workspace| {
        workspace.socket_rules.is_empty() && workspace.socket_rule_created_order_high_water == 0
    }) {
        return Ok(());
    }
    Err(unsupported())
}

pub(crate) fn reject_workspace_fields(workspace: Option<&Value>) -> AppResult<()> {
    if workspace.and_then(Value::as_object).is_some_and(|object| {
        object.contains_key("socket_rules")
            || object.contains_key("socket_rule_created_order_high_water")
    }) {
        return Err(unsupported());
    }
    Ok(())
}

pub(crate) fn remove_workspace_fields(workspace: Option<&mut Value>) {
    if let Some(object) = workspace.and_then(Value::as_object_mut) {
        object.remove("socket_rules");
        object.remove("socket_rule_created_order_high_water");
    }
}

pub(crate) fn reject_configuration_fields(value: &Value) -> AppResult<()> {
    if let Some(workspaces) = value.get("workspaces").and_then(Value::as_array) {
        for workspace in workspaces {
            reject_workspace_fields(Some(workspace))?;
        }
    }
    Ok(())
}

pub(crate) fn remove_configuration_fields(value: &mut Value) {
    if let Some(workspaces) = value.get_mut("workspaces").and_then(Value::as_array_mut) {
        for workspace in workspaces {
            remove_workspace_fields(Some(workspace));
        }
    }
}

fn unsupported() -> AppError {
    AppError::new("SOCKET_RULE_PORTABILITY_REQUIRES_V4", UNSUPPORTED_MESSAGE)
}
