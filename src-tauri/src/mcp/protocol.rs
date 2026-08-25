//! Official `rmcp` handler for MCP protocol revision 2026-07-28.

use std::{borrow::Cow, sync::Arc, time::Duration};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, Implementation,
        ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ResourceContents,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::{MaybeSendFuture, RequestContext},
};
use serde_json::{Value, json};
use tokio::time::timeout;

use super::{backend::ReadOnlyMcpBackend, catalog, resources};

#[cfg(test)]
pub const PROTOCOL_VERSION: &str = "2026-07-28";
pub const MAX_TOOL_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_LOGICAL_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const TOOL_DEADLINE: Duration = Duration::from_secs(8);
const RESOURCE_TTL_MS: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct ReadOnlyMcpHandler {
    backend: Arc<dyn ReadOnlyMcpBackend>,
}

impl ReadOnlyMcpHandler {
    pub fn new(backend: Arc<dyn ReadOnlyMcpBackend>) -> Self {
        Self { backend }
    }

    async fn execute_tool(&self, name: &str, arguments: Value) -> CallToolResult {
        if logical_bytes(&arguments) > MAX_TOOL_INPUT_BYTES {
            return CallToolResult::structured_error(json!({
                "code": "INPUT_BUDGET_EXCEEDED",
                "message": format!("tool arguments exceed {MAX_TOOL_INPUT_BYTES} logical JSON bytes")
            }));
        }
        if let Err(message) = catalog::validate_arguments(name, &arguments) {
            return CallToolResult::structured_error(json!({
                "code": "INVALID_ARGUMENTS",
                "message": message
            }));
        }
        match timeout(TOOL_DEADLINE, self.backend.call_tool(name, arguments)).await {
            Ok(Ok(value)) => {
                if let Err(message) = catalog::validate_successful_output(name, &value) {
                    return CallToolResult::structured_error(json!({
                        "code": "OUTPUT_SCHEMA_MISMATCH",
                        "message": message
                    }));
                }
                bounded_result(value, false)
            }
            Ok(Err(failure)) => bounded_result(failure.as_value(), true),
            Err(_) => CallToolResult::structured_error(json!({
                "code": "TOOL_DEADLINE_EXCEEDED",
                "message": format!("tool did not complete within {} seconds", TOOL_DEADLINE.as_secs())
            })),
        }
    }

    async fn execute_resource(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        let value = timeout(TOOL_DEADLINE, self.backend.read_resource(uri))
            .await
            .map_err(|_| ErrorData::internal_error("resource deadline exceeded", None))?
            .map_err(|failure| {
                ErrorData::resource_not_found(failure.message.clone(), Some(failure.as_value()))
            })?;
        if logical_bytes(&value) > MAX_LOGICAL_OUTPUT_BYTES {
            return Err(ErrorData::internal_error(
                "resource exceeds MCP logical output budget",
                None,
            ));
        }
        let content = serde_json::from_value::<ResourceContents>(value)
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![content])
            .with_ttl_ms(RESOURCE_TTL_MS)
            .with_cache_scope(CacheScope::Private))
    }
}

impl ServerHandler for ReadOnlyMcpHandler {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::V_2026_07_28)
        .with_server_info(Implementation::new(
            "intercept-proxy-read-only",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Loopback-only, unauthenticated, read-only diagnostics. Any local process can read exposed data. Use tools and resources to separate observed evidence from hypotheses, explain certificate and protocol concepts for beginners, and propose App-side changes with alternatives, risks, rollback and verification. No tool mutates the App, configuration, runtime, files, captures, rules or protocol packages.",
        )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(catalog::tools())
            .with_ttl_ms(RESOURCE_TTL_MS)
            .with_cache_scope(CacheScope::Private)))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        catalog::tools().into_iter().find(|tool| tool.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.get_tool(request.name.as_ref()).is_none() {
            return Err(ErrorData::invalid_params(
                format!("unknown read-only tool: {}", request.name),
                None,
            ));
        }
        let arguments = request.arguments.map_or_else(|| json!({}), Value::Object);
        Ok(self
            .execute_tool(request.name.as_ref(), arguments)
            .await
            .into())
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(resources::list())
            .with_ttl_ms(RESOURCE_TTL_MS)
            .with_cache_scope(CacheScope::Private)))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        Ok(self.execute_resource(&request.uri).await?.into())
    }
}

fn bounded_result(value: Value, is_error: bool) -> CallToolResult {
    if logical_bytes(&value) > MAX_LOGICAL_OUTPUT_BYTES {
        return CallToolResult::structured_error(json!({
            "code": "OUTPUT_BUDGET_EXCEEDED",
            "message": format!("tool result exceeds {MAX_LOGICAL_OUTPUT_BYTES} logical JSON bytes")
        }));
    }
    if is_error {
        CallToolResult::structured_error(value)
    } else {
        CallToolResult::structured(value)
    }
}

fn logical_bytes(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
pub(super) fn tools() -> Vec<Tool> {
    catalog::tools()
}
