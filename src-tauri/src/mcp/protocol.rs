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

use super::{
    backend::{McpBackend, McpCallContext},
    catalog,
    environment_contract::{EnvironmentToolKind, environment_tool_kind},
    resources,
    server::McpTransportCapabilities,
};

#[cfg(test)]
pub const PROTOCOL_VERSION: &str = "2026-07-28";
const READ_INPUT_BYTES: usize = 256 * 1024;
const READ_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const READ_DEADLINE: Duration = Duration::from_secs(8);
const CREATE_INPUT_BYTES: usize = 1024 * 1024;
const CREATE_OUTPUT_BYTES: usize = 1024 * 1024;
const CREATE_DEADLINE: Duration = Duration::from_secs(30);
const STATUS_CANCEL_APPLY_INPUT_BYTES: usize = 16 * 1024;
const STATUS_CANCEL_APPLY_OUTPUT_BYTES: usize = 1024 * 1024;
const STATUS_CANCEL_APPLY_DEADLINE: Duration = Duration::from_secs(8);
#[cfg(test)]
pub const MAX_TOOL_INPUT_BYTES: usize = READ_INPUT_BYTES;
pub const MAX_LOGICAL_OUTPUT_BYTES: usize = READ_OUTPUT_BYTES;
const RESOURCE_TTL_MS: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct McpHandler {
    backend: Arc<dyn McpBackend>,
    transport_capabilities: Arc<McpTransportCapabilities>,
}

impl McpHandler {
    pub fn new(
        backend: Arc<dyn McpBackend>,
        transport_capabilities: Arc<McpTransportCapabilities>,
    ) -> Self {
        Self {
            backend,
            transport_capabilities,
        }
    }

    async fn execute_tool(
        &self,
        name: &str,
        arguments: Value,
        context: McpCallContext,
    ) -> CallToolResult {
        let policy = tool_policy(name);
        if logical_bytes(&arguments) > policy.input_bytes {
            let code = environment_tool_kind(name)
                .map_or("INPUT_BUDGET_EXCEEDED", |_| "MCP_TOOL_ARGUMENTS_INVALID");
            return CallToolResult::structured_error(json!({
                "code": code,
                "message": format!("tool arguments exceed {} logical JSON bytes", policy.input_bytes)
            }));
        }
        if let Err(message) = catalog::validate_arguments(name, &arguments) {
            let code = environment_tool_kind(name)
                .map_or("INVALID_ARGUMENTS", |_| "MCP_TOOL_ARGUMENTS_INVALID");
            return CallToolResult::structured_error(json!({
                "code": code,
                "message": message
            }));
        }
        let call = self
            .backend
            .call_tool_with_context(name, arguments, context);
        let result = if environment_tool_kind(name) == Some(EnvironmentToolKind::Create) {
            // Application owns the create deadline and registry cleanup. An equal outer timeout
            // could drop that future before it publishes the terminal result and clears private
            // candidate bytes.
            call.await
        } else {
            match timeout(policy.deadline, call).await {
                Ok(result) => result,
                Err(_) => {
                    return CallToolResult::structured_error(json!({
                        "code": deadline_code(name),
                        "message": format!("tool did not complete within {} seconds", policy.deadline.as_secs())
                    }));
                }
            }
        };
        match result {
            Ok(value) => {
                if let Err(message) = catalog::validate_successful_output(name, &value) {
                    return CallToolResult::structured_error(json!({
                        "code": "OUTPUT_SCHEMA_MISMATCH",
                        "message": message
                    }));
                }
                bounded_result(value, false, policy.output_bytes, name)
            }
            Err(failure) => bounded_result(failure.as_value(), true, policy.output_bytes, name),
        }
    }

    async fn execute_resource(&self, uri: &str) -> Result<ReadResourceResult, ErrorData> {
        let value = timeout(READ_DEADLINE, self.backend.read_resource(uri))
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

impl ServerHandler for McpHandler {
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
            "intercept-proxy",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "All-interface plaintext HTTP with no client authentication or authorization. Any host that can reach this port can read exposed data and use the environment candidate tools to validate, preview, cancel, and atomically apply full Workspace configuration. Treat the transport as intentionally insecure: private material and confirmation tokens cross the network in plaintext. Existing diagnostics remain read-only; write tools operate only through the typed candidate and Application mutation boundary and never auto-stop or restart Listeners.",
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
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.get_tool(request.name.as_ref()).is_none() {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                None,
            ));
        }
        let arguments = request.arguments.map_or_else(|| json!({}), Value::Object);
        let context = McpCallContext {
            request_cancellation: context.ct,
            transport_capabilities: Arc::clone(&self.transport_capabilities),
        };
        Ok(self
            .execute_tool(request.name.as_ref(), arguments, context)
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

fn bounded_result(value: Value, is_error: bool, output_bytes: usize, name: &str) -> CallToolResult {
    if logical_bytes(&value) > output_bytes {
        let code = environment_tool_kind(name)
            .map_or("OUTPUT_BUDGET_EXCEEDED", |_| "MCP_PROTOCOL_INVALID");
        return CallToolResult::structured_error(json!({
            "code": code,
            "message": format!("tool result exceeds {output_bytes} logical JSON bytes")
        }));
    }
    if is_error {
        CallToolResult::structured_error(value)
    } else {
        CallToolResult::structured(value)
    }
}

#[derive(Clone, Copy)]
struct ToolPolicy {
    input_bytes: usize,
    output_bytes: usize,
    deadline: Duration,
}

fn tool_policy(name: &str) -> ToolPolicy {
    match environment_tool_kind(name) {
        Some(EnvironmentToolKind::Create) => ToolPolicy {
            input_bytes: CREATE_INPUT_BYTES,
            output_bytes: CREATE_OUTPUT_BYTES,
            deadline: CREATE_DEADLINE,
        },
        Some(
            EnvironmentToolKind::Status | EnvironmentToolKind::Cancel | EnvironmentToolKind::Apply,
        ) => ToolPolicy {
            input_bytes: STATUS_CANCEL_APPLY_INPUT_BYTES,
            output_bytes: STATUS_CANCEL_APPLY_OUTPUT_BYTES,
            deadline: STATUS_CANCEL_APPLY_DEADLINE,
        },
        Some(EnvironmentToolKind::Capabilities) | None => ToolPolicy {
            input_bytes: READ_INPUT_BYTES,
            output_bytes: READ_OUTPUT_BYTES,
            deadline: READ_DEADLINE,
        },
    }
}

fn deadline_code(name: &str) -> &'static str {
    match environment_tool_kind(name) {
        Some(_) => "MCP_PROTOCOL_INVALID",
        None => "TOOL_DEADLINE_EXCEEDED",
    }
}

fn logical_bytes(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
pub(super) fn tools() -> Vec<Tool> {
    catalog::tools()
}
