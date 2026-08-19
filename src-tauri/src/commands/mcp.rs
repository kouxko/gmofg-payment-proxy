//! 内嵌只读 MCP 的运行信息。这里只投影组合根状态，不读取业务仓储。

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::{
    app_state::AppState,
    mcp::{MCP_ENDPOINT, catalog_size},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct McpInfoViewModel {
    pub available: bool,
    pub endpoint: String,
    pub protocol_version: String,
    pub transport: String,
    pub access_scope: String,
    pub authentication: String,
    pub tool_count: usize,
    pub resource_count: usize,
}

#[tauri::command]
#[specta::specta]
// Tauri 的命令参数提取器按值传入 `State`；函数本身只读取其中的组合根状态。
#[allow(clippy::needless_pass_by_value)]
pub fn mcp_info(state: State<'_, AppState>) -> McpInfoViewModel {
    let (tool_count, resource_count) = catalog_size();
    McpInfoViewModel {
        available: state.mcp().is_some(),
        endpoint: MCP_ENDPOINT.to_owned(),
        protocol_version: "2026-07-28".to_owned(),
        transport: "Streamable HTTP".to_owned(),
        access_scope: "仅本机 127.0.0.1，只读".to_owned(),
        authentication: "无认证；同机进程可读取".to_owned(),
        tool_count,
        resource_count,
    }
}
