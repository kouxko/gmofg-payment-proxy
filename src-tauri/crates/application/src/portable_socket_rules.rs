//! Socket 规则在 v3 可移植文档中的只读兼容边界。
//!
//! v4 已正式携带规则；解析 v3 时仍必须先拒绝伪造的规则和高水位字段，避免攻击者
//! 借助当前 `ProxyWorkspace` 的 Serde 默认值把新版状态偷偷塞进旧 wire。

use serde_json::Value;

use crate::{AppError, AppResult};

const UNSUPPORTED_MESSAGE: &str =
    "当前 v3 可移植格式不支持 Socket 规则，请使用支持 Socket 规则的新版可移植格式。";

pub(crate) fn reject_workspace_fields(workspace: Option<&Value>) -> AppResult<()> {
    if workspace.and_then(Value::as_object).is_some_and(|object| {
        object.contains_key("socket_rules")
            || object.contains_key("socket_rule_created_order_high_water")
    }) {
        return Err(unsupported());
    }
    Ok(())
}

pub(crate) fn reject_configuration_fields(value: &Value) -> AppResult<()> {
    if let Some(workspaces) = value.get("workspaces").and_then(Value::as_array) {
        for workspace in workspaces {
            reject_workspace_fields(Some(workspace))?;
        }
    }
    Ok(())
}

fn unsupported() -> AppError {
    AppError::new("SOCKET_RULE_PORTABILITY_REQUIRES_V4", UNSUPPORTED_MESSAGE)
}
