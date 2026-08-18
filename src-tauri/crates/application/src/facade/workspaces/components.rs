//! 未持久化 Workspace 组件草稿的领域编辑意图。

use intercept_proxy_domain::{
    ConnectionFaultAction, FaultPreset, FaultPresetId, ResponseAssertion, ResponseAssertionId,
    ResponseAssertionKind,
};

use super::{
    AppError, AppResult, Application, ProxyWorkspace,
    support::{
        connection_fault, delete_component, find_mut, parse_listener_ids, response_assertion,
    },
};

impl Application {
    /// 在未保存的 Workspace 草稿中追加一个由 Rust 生成稳定 ID 的通用组件。
    ///
    /// 前端不得自行生成领域 ID；同时该命令不持久化草稿，调用者仍需执行
    /// `workspace_validate` 与 `workspace_save`。证书引用只能通过 Listener 证书导入端口
    /// 创建，不能用任意路径文本构造。
    pub fn workspace_component_new(
        &self,
        mut workspace: ProxyWorkspace,
        kind: &str,
    ) -> AppResult<ProxyWorkspace> {
        match kind {
            "response_assertion" => workspace.response_assertions.push(ResponseAssertion {
                id: ResponseAssertionId::new(),
                name: "Response Assertion".into(),
                listener_ids: Vec::new(),
                enabled: true,
                assertion: ResponseAssertionKind::HttpStatusEquals { expected: 200 },
            }),
            "fault_preset" => workspace.fault_presets.push(FaultPreset {
                id: FaultPresetId::new(),
                name: "Connection Fault Preset".into(),
                description: String::new(),
                connection_actions: vec![ConnectionFaultAction::Delay { milliseconds: 100 }],
                http_actions: Vec::new(),
            }),
            "certificate_reference" => {
                return Err(AppError::new(
                    "WORKSPACE_CERTIFICATE_IMPORT_REQUIRED",
                    "证书材料必须在代理入口中按用途导入。",
                ));
            }
            _ => {
                return Err(AppError::new(
                    "WORKSPACE_COMPONENT_KIND_INVALID",
                    "Workspace 组件类型无效。",
                ));
            }
        }
        Ok(workspace)
    }

    /// 对 Workspace 组件执行会改变领域结构的编辑意图。
    ///
    /// 前端只提交组件、意图和原始文本；联合类型默认值、Listener ID 解析与删除行为都
    /// 在 Rust 中完成，因此桌面 UI、未来 CLI/TUI 和无界面测试共享同一套语义。
    pub fn workspace_component_apply_intent(
        &self,
        mut workspace: ProxyWorkspace,
        component_kind: &str,
        component_id: &str,
        operation: &str,
        value: &str,
    ) -> AppResult<ProxyWorkspace> {
        if operation == "delete" {
            delete_component(&mut workspace, component_kind, component_id)?;
            return Ok(workspace);
        }
        match (component_kind, operation) {
            ("response_assertion", "listener_ids") => {
                let ids = parse_listener_ids(value)?;
                find_mut(&mut workspace.response_assertions, component_id, |item| {
                    item.id.to_string()
                })?
                .listener_ids = ids;
            }
            ("response_assertion", "variant") => {
                find_mut(&mut workspace.response_assertions, component_id, |item| {
                    item.id.to_string()
                })?
                .assertion = response_assertion(value)?;
            }
            ("fault_preset", "variant") => {
                find_mut(&mut workspace.fault_presets, component_id, |item| {
                    item.id.to_string()
                })?
                .connection_actions = vec![connection_fault(value)?];
            }
            _ => {
                return Err(AppError::new(
                    "WORKSPACE_COMPONENT_INTENT_INVALID",
                    "Workspace 组件编辑意图无效。",
                ));
            }
        }
        Ok(workspace)
    }
}
