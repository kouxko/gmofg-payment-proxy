import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = process.argv[2] ? resolve(process.argv[2]) : resolve(import.meta.dirname, "..");
const files = {
  adr: "docs/architecture/decisions/ADR-009-nested-document-javascript-package-runtime.md",
  rootDocs: "docs/README.md",
  architecture: "docs/architecture/rules-and-protocol-packages.md",
  dataFlow: "docs/architecture/data-flow.md",
  modules: "docs/architecture/modules.md",
  user: "docs/user-operation-guide.md",
  onboarding: "docs/onboarding-guide.md",
  authoring: "templates/socket-protocol/AUTHORING.md",
  api: "templates/socket-protocol/API.md",
  mcpExternal: "docs/mcp/external-package-integration-guide.md",
  mcpDiagnostics: "docs/mcp/diagnostic-architecture.md",
  mcpTools: "docs/mcp/tool-reference.md",
  mcpValidation: "docs/mcp/validation-playbook.md",
  matrix: "docs/testing/release-validation-matrix.md",
  evidenceTemplate: "docs/testing/evidence/TEMPLATE.md",
  historicalTemplate: "docs/architecture/exchange-pipeline-template/README.md",
  resources: "src-tauri/src/mcp/resources.rs",
  catalog: "src-tauri/src/mcp/catalog.rs",
};

const sources = new Map();
const failures = [];
for (const [name, relative] of Object.entries(files)) {
  const path = resolve(root, relative);
  if (!existsSync(path)) {
    failures.push(`${name}: missing ${relative}`);
    sources.set(name, "");
  } else {
    sources.set(name, readFileSync(path, "utf8"));
  }
}

const required = [
  ["adr", "- Status: Accepted", "accepted replacement decision"],
  ["adr", "ADR-002", "superseded package decision link"],
  ["adr", "ADR-007", "superseded pipeline decision link"],
  ["adr", "manifest.json", "strict package manifest"],
  ["adr", "Boa", "JavaScript runtime owner"],
  ["adr", "Proxy -> Server", "upstream write boundary"],
  ["adr", "Proxy -> App", "downstream write boundary"],
  ["rootDocs", "ADR-002：HTTP 协议包（已被 ADR-009 替代）", "ADR-002 historical index state"],
  ["rootDocs", "ADR-007：Exchange/Pipeline 运行边界（已被 ADR-009 替代）", "ADR-007 historical index state"],
  ["rootDocs", "ADR-009：递归 Document、统一规则与 JavaScript 协议包运行时", "ADR-009 discoverability"],
  ["rootDocs", "当前实现以 ADR-009、当前架构文档和源码为准", "current ADR authority"],
  ["rootDocs", "不再是 current authority", "historical template authority"],
  ["architecture", "递归 `Document`", "recursive Document contract"],
  ["architecture", "每条 condition 读取当前 working state", "working-state condition evaluation"],
  ["architecture", "供后序规则条件观察", "earlier changes visible to later rules"],
  ["architecture", "有序 action", "ordered action contract"],
  ["architecture", "只 Encode 一次", "single Encode boundary"],
  ["architecture", "`/packages`", "package WebSocket endpoint"],
  ["architecture", "Schema 100", "database compatibility boundary"],
  ["dataFlow", "Proxy -> Server", "upstream write stage"],
  ["dataFlow", "Proxy -> App", "downstream write stage"],
  ["modules", "Null", "Document null type"],
  ["modules", "Object", "Document object type"],
  ["modules", "Array", "Document array type"],
  ["user", "manifest.json", "user package manifest"],
  ["user", "JavaScript", "user package language"],
  ["user", "递归", "user nested Document guidance"],
  ["user", "installed + enabled", "post-commit package failure retention"],
  ["user", "不会自动回滚、重试", "package failure no rollback or retry"],
  ["onboarding", "Boa", "onboarding runtime"],
  ["onboarding", "`/packages`", "onboarding package endpoint"],
  ["onboarding", "current working Document", "onboarding working-state rules"],
  ["authoring", "manifest.json", "authoring manifest"],
  ["authoring", "Uint8Array", "authoring binary wire"],
  ["authoring", "canonical padded Base64", "public package binary wire"],
  ["authoring", "current Boa host", "precise Boa host surface"],
  ["authoring", "later rules observe earlier changes", "authoring working-state visibility"],
  ["api", "package.register", "registration method"],
  ["api", "hooks.upstream.encode", "upstream Encode hook"],
  ["api", "document.downstream.display", "downstream display hook"],
  ["api", "Local Boa exports", "local Boa export boundary"],
  ["api", "Public `/packages` JSON-RPC", "public package RPC boundary"],
  ["api", "canonical padded Base64", "public RPC Base64 wire"],
  ["api", "without an `id`", "registration notification without id"],
  ["api", "Proxy sends no response", "registration notification no response"],
  ["api", "complete Manifest", "registration params manifest"],
  ["api", "string `id`", "response string id"],
  ["api", "error.data.code", "stable RPC machine code"],
  ["api", "canonical padded Base64", "canonical padded Socket wire"],
  ["mcpExternal", "`/packages`", "MCP package endpoint"],
  ["mcpExternal", "package.register", "MCP registration method"],
  ["mcpExternal", "首次远端注册", "first remote registration lifecycle"],
  ["mcpExternal", "并按合同启用", "first remote registration enabled state"],
  ["mcpExternal", "max_body_bytes", "transport body budget owner"],
  ["mcpExternal", "canonical padded Base64", "external guide padded Base64"],
  ["mcpDiagnostics", "stable code", "stable failure evidence"],
  ["mcpDiagnostics", "changes_truncated", "bounded process evidence"],
  ["mcpTools", "protocol-package-authoring", "authoring resource"],
  ["mcpValidation", "NOT_RUN", "unexecuted validation state"],
  ["mcpValidation", "canonical padded Base64", "validation padded Base64"],
  ["matrix", "JavaScript", "JavaScript validation row"],
  ["matrix", "Proxy -> Server", "upstream validation stage"],
  ["matrix", "Proxy -> App", "downstream validation stage"],
  ["matrix", "NOT_RUN", "explicit unrun result"],
  ["matrix", "canonical padded Base64", "matrix padded Base64"],
  ["matrix", "received/process/final/encoded", "matrix typed Document evidence"],
  ["evidenceTemplate", "changes_truncated", "typed process loss field"],
  ["evidenceTemplate", "processed.final_document", "final Document evidence"],
  ["evidenceTemplate", "NOT_RUN", "unrun evidence state"],
  ["historicalTemplate", "已由 [ADR-009]", "historical template supersession"],
  ["historicalTemplate", "不再是生产实现", "historical template authority removal"],
  ["resources", "RULES_AND_PROTOCOL_PACKAGES", "embedded architecture resource"],
  ["resources", "SOCKET_AUTHORING", "embedded authoring resource"],
];

for (const [owner, token, label] of required) {
  if (!sources.get(owner).includes(token)) failures.push(`${owner}: missing ${label}`);
}

const activeOwners = [
  "architecture",
  "dataFlow",
  "modules",
  "user",
  "onboarding",
  "authoring",
  "api",
  "mcpExternal",
  "mcpDiagnostics",
  "matrix",
];
const forbidden = [
  ["manifest.toml", "legacy TOML manifest"],
  ["protocol.rhai", "legacy Rhai entry point"],
  ["display.rhai", "legacy Rhai display entry point"],
  ["四阶段字段规则", "legacy four-stage rule model"],
  ["P-RHAI", "legacy Rhai release gate"],
  ["P-DENO", "non-authoritative Deno release gate"],
  ["Package code has no filesystem", "invented Boa sandbox guarantee"],
  ["所有规则针对原始 Document", "incorrect original-snapshot rule evaluation"],
  ["每个阶段的条件读取原始 Document", "incorrect original-snapshot stage evaluation"],
  ["match original Document", "incorrect original-snapshot onboarding"],
  ["against the original Document", "incorrect original-snapshot authoring"],
  ["不会隐式启用", "incorrect first-registration disabled state"],
  ["RPC timeout 或并发额度", "invented package settings"],
  ["普通 JSON-RPC wire message 上限", "invented ordinary RPC budget"],
  ["不向脚本暴露网络、文件、进程", "overstated Boa restriction"],
  ["loopback WebSocket `/packages`", "invented loopback package endpoint"],
  ["原始快照匹配", "incorrect original snapshot terminology"],
  ["兼容模式使用用户数据库", "invented database compatibility mode"],
  ["详情只显示固定的 Display HTML", "typed Document evidence hidden"],
  ["上下行 hook、断线、重连、超时、限额", "invented hook timeout and limit"],
  ["调用顺序和超时预算", "invented hook timeout budget"],
  ["当前实现以 ADR-007 和源码为准", "obsolete ADR-007 current authority"],
];
for (const owner of activeOwners) {
  for (const [token, label] of forbidden) {
    if (sources.get(owner).includes(token)) failures.push(`${owner}: contains ${label}`);
  }
}

if (sources.get("historicalTemplate").includes("权威设计模板")) {
  failures.push("historicalTemplate: still claims current authority");
}
const toolCount = [...sources.get("catalog").matchAll(/\btool\(\s*\n?\s*"([a-z0-9_]+)"/gu)].length;
if (toolCount !== 36) failures.push(`catalog: expected 36 read tools, found ${toolCount}`);
for (const owner of ["user", "onboarding", "dataFlow"]) {
  if (!sources.get(owner).includes("36 个")) failures.push(`${owner}: missing current 36-read-tool count`);
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("phase16 current documentation contract: PASS");
