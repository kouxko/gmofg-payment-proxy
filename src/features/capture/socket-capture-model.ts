import type {
  DocumentSchema,
  ProtocolPackageRef,
  SocketCaptureDetailViewModel,
  SocketCaptureDocument,
  SocketCapturePageViewModel,
  SocketCaptureQuery,
  SocketCaptureRowViewModel,
  SocketCaptureSchemaRef,
  OperationResultViewModel,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";

/** Socket 查询只使用后端公开的 Socket 维度，绝不复用 HTTP 查询条件。 */
export function defaultSocketCaptureQuery(workspaceId: string): SocketCaptureQuery {
  return {
    workspace_id: workspaceId,
    listener_id: null,
    session_id: null,
    connection_id: null,
    package: null,
    direction: null,
    kind: null,
    occurred_from: null,
    occurred_to: null,
    sort: "occurred_at",
    direction_sort: "desc",
    page: { page: 1, page_size: 50 },
  };
}

export function packageLabel(value: ProtocolPackageRef): string {
  return `${value.id}@${value.version}`;
}

export function schemaLabel(value: SocketCaptureSchemaRef): string {
  return `${value.id} v${value.version}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isNonnegativeSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

function isTimestamp(value: unknown): value is string {
  return isText(value) && Number.isFinite(Date.parse(value));
}

function hasOnly(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const accepted = new Set(keys);
  return Object.keys(value).every((key) => accepted.has(key));
}

function isBytes(value: unknown): value is number[] {
  return Array.isArray(value)
    && value.every((item) => Number.isInteger(item) && item >= 0 && item <= 255);
}

function samePackage(left: unknown, right: ProtocolPackageRef): boolean {
  return isRecord(left) && hasOnly(left, ["id", "version"])
    && left.id === right.id && left.version === right.version;
}

function sameSchemaRef(left: unknown, right: SocketCaptureSchemaRef): boolean {
  return isRecord(left) && hasOnly(left, ["id", "version"])
    && left.id === right.id && left.version === right.version;
}

function isI64Text(value: unknown): value is string {
  if (typeof value !== "string" || !/^(?:0|[1-9]\d*|-[1-9]\d*)$/.test(value)) return false;
  const parsed = BigInt(value);
  return parsed >= BigInt("-9223372036854775808")
    && parsed <= BigInt("9223372036854775807");
}

function isSchema(value: unknown, expected: SocketCaptureSchemaRef): value is DocumentSchema {
  if (!isRecord(value) || !hasOnly(value, ["id", "version", "title", "fields"])
    || value.id !== expected.id || value.version !== expected.version
    || !isText(value.title) || !Array.isArray(value.fields) || value.fields.length === 0) return false;
  const names = new Set<string>();
  return value.fields.every((field) => {
    if (!isRecord(field) || !hasOnly(field, ["name", "type", "label"])
      || !isText(field.name) || typeof field.label !== "string"
      || !["string", "int", "bool", "blob"].includes(String(field.type))
      || names.has(field.name)) return false;
    names.add(field.name);
    return true;
  });
}

function isDocument(value: unknown, expected: SocketCaptureSchemaRef): value is SocketCaptureDocument {
  if (!isRecord(value) || !hasOnly(value, ["schema", "values"])
    || !isSchema(value.schema, expected) || !Array.isArray(value.values)
    || value.values.length !== value.schema.fields.length) return false;
  const schema = value.schema;
  return value.values.every((item, index) => {
    if (item === null || !isRecord(item)) return item === null;
    if (!hasOnly(item, ["type", "value"])) return false;
    const field = schema.fields[index];
    if (item.type !== field.type) return false;
    if (item.type === "string") return typeof item.value === "string";
    if (item.type === "int") return isI64Text(item.value);
    if (item.type === "bool") return typeof item.value === "boolean";
    return item.type === "blob" && isBytes(item.value);
  });
}

function isRuleIds(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(isText) && new Set(value).size === value.length;
}

function sameRuleIds(value: unknown, expected: string[]): boolean {
  return isRuleIds(value) && value.length === expected.length
    && value.every((id, index) => id === expected[index]);
}

function sameLocalRuleIds(request: unknown, response: unknown, expected: string[]): boolean {
  return isRuleIds(request) && isRuleIds(response)
    && sameRuleIds([...request, ...response], expected);
}

function isDisplay(value: unknown): boolean {
  if (!isRecord(value)) return false;
  if (value.type === "untrusted_html") {
    return hasOnly(value, ["type", "html"]) && typeof value.html === "string";
  }
  if (value.type !== "hex_fallback" || !hasOnly(value, ["type", "reason", "diagnostic"])
    || !["entry_point_failed", "resource_limit_exceeded"].includes(String(value.reason))) return false;
  return value.diagnostic === null || (isRecord(value.diagnostic)
    && hasOnly(value.diagnostic, ["code", "message"])
    && isText(value.diagnostic.code) && typeof value.diagnostic.message === "string");
}

const localFailureMessages = {
  response_rule: "代理→应用规则执行失败。",
  response_encode: "响应报文生成失败，请检查代理→应用规则是否补齐协议要求的字段。",
  response_write: "响应写回应用失败，已保留请求解析结果和已写出的响应前缀。",
} as const;

function isLocalFailure(value: unknown): boolean {
  if (!isRecord(value) || !hasOnly(value, ["stage", "code", "message"])
    || !isText(value.code) || !isText(value.message)
    || !Object.hasOwn(localFailureMessages, String(value.stage))) return false;
  const stage = value.stage as keyof typeof localFailureMessages;
  return value.message === localFailureMessages[stage];
}

function isRelayCapture(value: unknown, row: SocketCaptureRowViewModel): boolean {
  if (!isRecord(value) || !hasOnly(value, ["direction", "package", "schema", "origin", "stages", "written", "display"])
    || value.direction !== row.direction || !samePackage(value.package, row.package)
    || !sameSchemaRef(value.schema, row.schema) || !isBytes(value.origin) || !isBytes(value.written)
    || !Array.isArray(value.stages) || !isDisplay(value.display)) return false;
  if (value.origin.length !== row.origin_size_bytes || value.written.length !== row.written_size_bytes) return false;
  const expectedStages = row.direction === "upstream"
    ? ["app_to_proxy", "proxy_to_upstream"]
    : ["upstream_to_proxy", "proxy_to_app"];
  const stagesValid = value.stages.length === 2 && value.stages.every((stage, index) =>
    isRecord(stage) && hasOnly(stage, ["stage", "matched_rule_ids", "document"])
      && stage.stage === expectedStages[index] && isRuleIds(stage.matched_rule_ids)
      && isDocument(stage.document, row.schema));
  if (!stagesValid) return false;
  const matched = value.stages.flatMap((stage) => (stage as { matched_rule_ids: string[] }).matched_rule_ids);
  return sameRuleIds(matched, row.matched_rule_ids);
}

function isLocalCapture(value: unknown, row: SocketCaptureRowViewModel): boolean {
  if (!isRecord(value) || !hasOnly(value, ["exchange_id", "package", "request_schema", "response_schema", "request_origin", "request_document", "request_display", "response_document", "matched_request_rule_ids", "matched_response_rule_ids", "written_response", "response_display"])
    || row.direction !== null || !isText(value.exchange_id)
    || !samePackage(value.package, row.package) || !isSchemaRef(value.request_schema)
    || !sameSchemaRef(value.response_schema, row.schema)
    || !isBytes(value.request_origin) || !isBytes(value.written_response)
    || !sameLocalRuleIds(value.matched_request_rule_ids, value.matched_response_rule_ids, row.matched_rule_ids)
    || !isDisplay(value.response_display)
    || !isDisplay(value.request_display)
    || !isDocument(value.request_document, value.request_schema as SocketCaptureSchemaRef)
    || !isDocument(value.response_document, value.response_schema as SocketCaptureSchemaRef)) return false;
  if (value.request_origin.length !== row.origin_size_bytes
    || value.written_response.length !== row.written_size_bytes) return false;
  return true;
}

function isLocalFailureCapture(value: unknown, row: SocketCaptureRowViewModel): boolean {
  if (!isRecord(value) || !row.failure || !hasOnly(value, [
    "exchange_id", "package", "request_schema", "response_schema", "request_origin",
    "request_document", "request_display", "matched_request_rule_ids",
    "matched_response_rule_ids", "response_document", "failure_stage", "failure_code",
    "failure_message", "written_response_prefix",
  ]) || row.direction !== null || !isText(value.exchange_id)
    || !samePackage(value.package, row.package) || !isSchemaRef(value.request_schema)
    || !sameSchemaRef(value.response_schema, row.schema)
    || !isBytes(value.request_origin) || !isBytes(value.written_response_prefix)
    || !sameLocalRuleIds(value.matched_request_rule_ids, value.matched_response_rule_ids, row.matched_rule_ids)
    || !isDisplay(value.request_display)
    || !isDocument(value.request_document, value.request_schema as SocketCaptureSchemaRef)
    || !(value.response_document === null
      || isDocument(value.response_document, value.response_schema as SocketCaptureSchemaRef))
    || value.failure_stage !== row.failure.stage || value.failure_code !== row.failure.code
    || value.failure_message !== row.failure.message) return false;
  return value.request_origin.length === row.origin_size_bytes
    && value.written_response_prefix.length === row.written_size_bytes;
}

function isPackage(value: unknown): value is ProtocolPackageRef {
  return isRecord(value) && hasOnly(value, ["id", "version"])
    && isText(value.id) && isText(value.version);
}

function isSchemaRef(value: unknown): value is SocketCaptureSchemaRef {
  return isRecord(value) && hasOnly(value, ["id", "version"])
    && isText(value.id) && Number.isSafeInteger(value.version) && Number(value.version) >= 1;
}

function isCaptureRow(value: unknown, workspaceId: string): value is SocketCaptureRowViewModel {
  if (!isRecord(value) || !hasOnly(value, ["capture_id", "runtime_epoch", "session_id", "connection_id", "listener_id", "occurred_at", "completed_at", "kind", "direction", "package", "schema", "origin_size_bytes", "written_size_bytes", "logical_size_bytes", "matched_rule_ids", "failure"])) return false;
  const sizes = [value.origin_size_bytes, value.written_size_bytes, value.logical_size_bytes];
  return isText(value.capture_id) && isText(value.runtime_epoch) && isText(value.session_id)
    && value.session_id === value.connection_id
    && isText(value.connection_id) && isText(value.listener_id) && isTimestamp(value.occurred_at)
    && isTimestamp(value.completed_at) && Date.parse(value.completed_at) >= Date.parse(value.occurred_at)
    && ["relay_frame", "local_exchange"].includes(String(value.kind))
    && (value.kind === "relay_frame"
      ? value.direction === "upstream" || value.direction === "downstream"
      : value.direction === null)
    && isPackage(value.package) && isSchemaRef(value.schema) && sizes.every(isNonnegativeSafeInteger)
    && Number(value.origin_size_bytes) > 0
    && (value.failure === null
      ? Number(value.written_size_bytes) > 0
      : value.kind === "local_exchange" && isLocalFailure(value.failure))
    && (value.kind === "relay_frame" ? value.failure === null : true)
    && isRuleIds(value.matched_rule_ids) && workspaceId.length > 0;
}

export function validateSocketCapturePage(value: unknown, workspaceId: string): SocketCapturePageViewModel | undefined {
  if (!isRecord(value) || !hasOnly(value, ["rows", "total", "page", "page_size", "total_pages", "empty_message"])
    || !Array.isArray(value.rows) || !value.rows.every((row) => isCaptureRow(row, workspaceId))
    || ![value.total, value.page, value.page_size, value.total_pages].every(Number.isSafeInteger)
    || Number(value.total) < 0 || Number(value.page) < 1 || Number(value.page_size) < 1
    || Number(value.total_pages) < 0 || typeof value.empty_message !== "string") return undefined;
  const ids = new Set((value.rows as SocketCaptureRowViewModel[]).map((row) => row.capture_id));
  const expectedPages = Math.ceil(Number(value.total) / Number(value.page_size));
  const offset = (Number(value.page) - 1) * Number(value.page_size);
  if (!Number.isSafeInteger(offset)) return undefined;
  const expectedRows = offset >= Number(value.total)
    ? 0
    : Math.min(Number(value.page_size), Number(value.total) - offset);
  if (ids.size !== value.rows.length || value.rows.length > Number(value.page_size)
    || value.rows.length !== expectedRows
    || Number(value.total_pages) !== expectedPages
    || (Number(value.page) > Math.max(1, expectedPages) && value.rows.length > 0)) return undefined;
  return value as SocketCapturePageViewModel;
}

export function validateSelectedWorkspace(value: unknown): WorkspaceSummaryViewModel | undefined {
  if (!Array.isArray(value)) return undefined;
  const valid = value.every((item) => isRecord(item)
    && hasOnly(item, ["id", "name", "revision", "listener_count", "enabled_listener_count", "selected"])
    && isText(item.id) && isText(item.name) && isNonnegativeSafeInteger(item.revision)
    && isNonnegativeSafeInteger(item.listener_count)
    && isNonnegativeSafeInteger(item.enabled_listener_count)
    && Number(item.enabled_listener_count) <= Number(item.listener_count)
    && typeof item.selected === "boolean");
  if (!valid) return undefined;
  const summaries = value as WorkspaceSummaryViewModel[];
  if (new Set(summaries.map((item) => item.id)).size !== summaries.length) return undefined;
  const selected = summaries.filter((item) => item.selected);
  return selected.length === 1 ? selected[0] : undefined;
}

export function validateOperationResult(value: unknown): OperationResultViewModel | undefined {
  if (!isRecord(value) || !hasOnly(value, ["success", "cancelled", "message", "ui_tone", "entity_id", "revision", "requires_restart"])
    || typeof value.success !== "boolean" || typeof value.cancelled !== "boolean"
    || typeof value.message !== "string" || !["neutral", "info", "positive", "warning", "danger"].includes(String(value.ui_tone))
    || !(value.entity_id === null || isText(value.entity_id))
    || !(value.revision === null || isNonnegativeSafeInteger(value.revision))
    || typeof value.requires_restart !== "boolean") return undefined;
  return value as OperationResultViewModel;
}

/**
 * IPC 数据仍被视为不可信输入。详情必须与当前选中行、工作区以及封闭联合分支
 * 完全一致；任何错配都 fail-closed，避免把上一条或畸形报文展示在当前标题下。
 */
export function validateSocketCaptureDetail(
  value: unknown,
  row: SocketCaptureRowViewModel,
  workspaceId: string,
): SocketCaptureDetailViewModel | undefined {
  if (!isRecord(value) || !isRecord(value.record)) return undefined;
  const record = value.record;
  if (!hasOnly(value, ["record"]) || !hasOnly(record, ["capture_id", "runtime_epoch", "workspace_id", "listener_id", "session_id", "connection_id", "peer_address", "occurred_at", "completed_at", "payload"])
    || record.capture_id !== row.capture_id || record.runtime_epoch !== row.runtime_epoch
    || record.workspace_id !== workspaceId || record.listener_id !== row.listener_id
    || record.session_id !== row.session_id || record.connection_id !== row.connection_id
    || record.session_id !== record.connection_id
    || !isText(record.peer_address) || record.occurred_at !== row.occurred_at
    || record.completed_at !== row.completed_at || !isRecord(record.payload)
    || !hasOnly(record.payload, ["kind", "capture"])
    || !isRecord(record.payload.capture)) return undefined;
  const valid = row.kind === "relay_frame"
    ? row.failure === null && record.payload.kind === "relay_frame"
      && isRelayCapture(record.payload.capture, row)
    : row.failure === null
      ? record.payload.kind === "local_exchange" && isLocalCapture(record.payload.capture, row)
      : record.payload.kind === "local_exchange_failure"
        && isLocalFailureCapture(record.payload.capture, row);
  return valid ? value as SocketCaptureDetailViewModel : undefined;
}
