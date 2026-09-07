//! 内嵌 MCP 的运行信息。这里只投影组合根状态，不读取业务仓储。

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::{
    app_state::AppState,
    mcp::{MCP_BIND_ENDPOINT, McpIpCapability, McpServer, catalog_size},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct McpIpCapabilityViewModel {
    pub available: bool,
    pub bind_address: String,
    pub port: u16,
    pub warning_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct McpInfoViewModel {
    pub available: bool,
    pub endpoint: String,
    pub protocol_version: String,
    pub transport: String,
    pub access_scope: String,
    pub authentication: String,
    pub plaintext_http: bool,
    pub ipv4: McpIpCapabilityViewModel,
    pub ipv6: McpIpCapabilityViewModel,
    pub warning_codes: Vec<String>,
    pub tool_count: usize,
    pub resource_count: usize,
}

#[tauri::command]
#[specta::specta]
// Tauri 的命令参数提取器按值传入 `State`；函数本身只读取其中的组合根状态。
#[allow(clippy::needless_pass_by_value)]
pub fn mcp_info(state: State<'_, AppState>) -> McpInfoViewModel {
    let (tool_count, resource_count) = catalog_size();
    let mcp = state.mcp();
    let capabilities = mcp.as_ref().map(McpServer::transport_capabilities);
    let project_ip = |capability: Option<&McpIpCapability>| McpIpCapabilityViewModel {
        available: capability.is_some_and(McpIpCapability::available),
        bind_address: capability.map_or_else(String::new, |value| value.bind_address().to_owned()),
        port: capability.map_or(17653, McpIpCapability::port),
        warning_codes: capability.map_or_else(Vec::new, |value| {
            value
                .warning_codes()
                .iter()
                .map(|code| code.as_str().to_owned())
                .collect()
        }),
    };
    let (ipv4, ipv6) = if let Some(capabilities) = capabilities.as_deref() {
        (
            project_ip(Some(capabilities.ipv4())),
            project_ip(Some(capabilities.ipv6())),
        )
    } else {
        (project_ip(None), project_ip(None))
    };
    McpInfoViewModel {
        available: mcp.is_some(),
        endpoint: MCP_BIND_ENDPOINT.to_owned(),
        protocol_version: "2026-07-28".to_owned(),
        transport: "Streamable HTTP（明文）".to_owned(),
        access_scope: "所有可达网络接口；客户端须把 0.0.0.0 替换为本机可达地址".to_owned(),
        authentication: "无认证；任何可达主机均可读取并修改 Proxy 配置".to_owned(),
        plaintext_http: true,
        ipv4,
        ipv6,
        warning_codes: capabilities.map_or_else(Vec::new, |value| {
            value
                .warnings()
                .iter()
                .map(|code| code.as_str().to_owned())
                .collect()
        }),
        tool_count,
        resource_count,
    }
}
