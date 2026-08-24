import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { productionRustSource } from "./rust-lexical-scan.mjs";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const proxySourceRoot = path.join(repositoryRoot, "src-tauri/crates/proxy/src");
const domainDocumentSourceRoot = path.join(repositoryRoot, "src-tauri/crates/domain/src/document");
const domainProtocolPackageSourceRoot = path.join(repositoryRoot, "src-tauri/crates/domain/src/protocol_package");
const domainSourceRoot = path.join(repositoryRoot, "src-tauri/crates/domain/src");
const exchangeSourceRoot = path.join(repositoryRoot, "src-tauri/crates/exchange/src");
const cratesSourceRoot = path.join(repositoryRoot, "src-tauri/crates");
const tauriAppSourceRoot = path.join(repositoryRoot, "src-tauri/src");
const exchangeCapabilityFile = path.join(exchangeSourceRoot, "capability.rs");
const sharedCapabilityHelperFiles = [path.join(proxySourceRoot, "transport/relay.rs")];
const listenerRuntimeSourceRoot = path.join(repositoryRoot, "src-tauri/crates/infrastructure/src/adapters/listener_runtime");
const applicationObservationFile = path.join(repositoryRoot, "src-tauri/crates/application/src/models/exchange_observation.rs");
const infrastructureObservationFile = path.join(repositoryRoot, "src-tauri/crates/infrastructure/src/adapters/exchange_observation.rs");
const runtimeObservationFile = path.join(repositoryRoot, "src-tauri/src/runtime_logs/exchange_ui_layer.rs");
const sqliteSourceRoot = path.join(repositoryRoot, "src-tauri/crates/infrastructure/src/sqlite");
const captureSourceRoot = path.join(repositoryRoot, "src/features/capture");

function matchingDelimiter(source, open, left, right) {
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === left) depth += 1;
    else if (source[index] === right && --depth === 0) return index;
  }
  return -1;
}

function splitTopLevel(source) {
  const parts = [];
  let start = 0;
  let depth = 0;
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    else if (source[index] === "}") depth -= 1;
    else if (source[index] === "," && depth === 0) {
      parts.push(source.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(source.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
}

function pathSegments(source) {
  return source
    .replace(/^::/u, "")
    .replace(/\s+as\s+[A-Za-z_][A-Za-z0-9_]*\s*$/u, "")
    .split(/\s*::\s*/u)
    .map((segment) => segment.trim())
    .filter((segment) => segment && segment !== "self");
}

function expandUseTree(tree, prefix = []) {
  const open = tree.indexOf("{");
  if (open < 0) return [[...prefix, ...pathSegments(tree)].join("::")];
  const close = matchingDelimiter(tree, open, "{", "}");
  if (close < 0) return [];
  const base = [...prefix, ...pathSegments(tree.slice(0, open))];
  return splitTopLevel(tree.slice(open + 1, close)).flatMap((child) => expandUseTree(child, base));
}

function normalizedRustImports(source) {
  return [...source.matchAll(/\buse\s+([^;]+);/gu)].flatMap((match) => expandUseTree(match[1]));
}

function normalizedRustPaths(source) {
  const imports = normalizedRustImports(source);
  const qualified = [...source.matchAll(/\b(?:crate|super|self|tauri|rhai|rusqlite|sqlx|hyper|hyper_util|http|http_body_util|intercept_proxy_[A-Za-z0-9_]+)(?:::[A-Za-z_][A-Za-z0-9_]*)+/gu)]
    .map((match) => match[0]);
  return [...new Set([...imports, ...qualified])];
}

function hasPath(imports, predicate) {
  return imports.some((entry) => predicate(entry.split("::")));
}

function includesSegment(segments, name) {
  return segments.includes(name);
}

function includesSequence(segments, sequence) {
  return segments.some((_, index) => sequence.every((part, offset) => segments[index + offset] === part));
}

function rustViolationCodes(role, source) {
  const inspected = productionRustSource(source);
  const imports = normalizedRustPaths(inspected);
  const failures = [];
  if (role === "http" && hasPath(imports, (parts) => includesSegment(parts, "socket_relay"))) {
    failures.push("HTTP_SOCKET_IMPORT");
  }
  if (role === "http" && hasPath(imports, (parts) => parts[0] === "intercept_proxy_domain"
    && (includesSegment(parts, "socket_document_rule") || parts.at(-1)?.startsWith("Socket")))) {
    failures.push("HTTP_SOCKET_CONTRACT_IMPORT");
  }
  if (role === "socket") {
    if (hasPath(imports, (parts) => ["hyper", "hyper_util", "http", "http_body_util"].includes(parts[0]))) {
      failures.push("SOCKET_HTTP_DTO_IMPORT");
    }
    if (hasPath(imports, (parts) => ["http", "message"].some((name) => includesSequence(parts, ["crate", name])))) {
      failures.push("SOCKET_HTTP_RUNTIME_IMPORT");
    }
    if (hasPath(imports, (parts) => includesSequence(parts, ["intercept_proxy_domain", "rule"]))) {
      failures.push("SOCKET_HTTP_RULE_IMPORT");
    }
  }
  if (role === "neutral") {
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_application")) failures.push("NEUTRAL_APPLICATION_UPWARD");
    if (hasPath(imports, (parts) => parts[0] === "tauri")) failures.push("NEUTRAL_UI_UPWARD");
  }
  if (role === "domain_document") {
    if (hasPath(imports, (parts) => includesSequence(parts, ["crate", "protocol_package"]))) failures.push("DOCUMENT_PROTOCOL_PACKAGE_DEPENDENCY");
    if (hasPath(imports, (parts) => includesSequence(parts, ["crate", "external_package"]))) failures.push("DOCUMENT_EXTERNAL_PACKAGE_DEPENDENCY");
    if (hasPath(imports, (parts) => includesSequence(parts, ["crate", "protocol_document_rule"])
      || includesSequence(parts, ["crate", "rule"]))) failures.push("DOCUMENT_RULE_DEPENDENCY");
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_application")) failures.push("DOCUMENT_APPLICATION_UPWARD");
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_infrastructure")) failures.push("DOCUMENT_INFRASTRUCTURE_UPWARD");
    if (hasPath(imports, (parts) => parts[0] === "tauri")) failures.push("DOCUMENT_TAURI_UPWARD");
    if (hasPath(imports, (parts) => parts[0] === "rhai")) failures.push("DOCUMENT_RHAI_DEPENDENCY");
  }
  if (role === "domain_protocol_package"
    && /\b(?:Document|DocumentField|DocumentFieldName|DocumentFieldType|DocumentSchema|DocumentSchemaId|DocumentValue)\b/u.test(inspected)) {
    failures.push("PROTOCOL_PACKAGE_DOCUMENT_TYPE");
  }
  if (role === "exchange") {
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_application")) failures.push("EXCHANGE_APPLICATION_UPWARD");
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_infrastructure")) failures.push("EXCHANGE_INFRASTRUCTURE_UPWARD");
    if (hasPath(imports, (parts) => parts[0] === "tauri")) failures.push("EXCHANGE_TAURI_UPWARD");
    if (hasPath(imports, (parts) => ["rusqlite", "sqlx"].includes(parts[0]) || includesSegment(parts, "sqlite"))) failures.push("EXCHANGE_SQLITE_DEPENDENCY");
    if (/\bStore\b/u.test(inspected)) failures.push("EXCHANGE_STORE_DEPENDENCY");
    if (/\bEventHub\b/u.test(inspected)) failures.push("EXCHANGE_EVENT_HUB_DEPENDENCY");
  }
  if (role === "domain_document_rules") {
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_application")) failures.push("DOCUMENT_RULE_APPLICATION_DEPENDENCY");
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_exchange")) failures.push("DOCUMENT_RULE_EXCHANGE_DEPENDENCY");
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_infrastructure")) failures.push("DOCUMENT_RULE_INFRASTRUCTURE_DEPENDENCY");
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_runtime")) failures.push("DOCUMENT_RULE_RUNTIME_DEPENDENCY");
    if (hasPath(imports, (parts) => parts[0] === "tauri")) failures.push("DOCUMENT_RULE_TAURI_DEPENDENCY");
    if (/\b(?:HttpConnectionIdentity|HttpContext|HttpDirectionCapabilities|SocketConnectionIdentity|SocketContext|SocketDirectionCapabilities|TcpStream|TlsStream)\b/u.test(inspected)) failures.push("DOCUMENT_RULE_TRANSPORT_TYPE");
  }
  if (role === "infra_http_document_rules"
    && /\b(?:SocketConnectionIdentity|SocketContext|SocketDirectionCapabilities|SocketProcessing[A-Za-z0-9_]*|SocketRelay[A-Za-z0-9_]*)\b/u.test(inspected)) {
    failures.push("HTTP_DOCUMENT_RULE_SOCKET_DEPENDENCY");
  }
  if (role === "infra_socket_document_rules"
    && /\b(?:HttpConnectionIdentity|HttpContext|HttpDirectionCapabilities)\b/u.test(inspected)) {
    failures.push("SOCKET_DOCUMENT_RULE_HTTP_DEPENDENCY");
  }
  if (role === "capability_non_owner"
    && /\b(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+(?:Frame|Decode|Display|Rules|Encode)\b/u.test(inspected)) {
    failures.push("CAPABILITY_STAGE_TRAIT_OUTSIDE_EXCHANGE");
  }
  if (role === "shared_capability_helper") {
    if (hasPath(imports, (parts) => ["hyper", "hyper_util", "http", "http_body_util"].includes(parts[0])
      || includesSequence(parts, ["crate", "http"]))) failures.push("SHARED_CAPABILITY_HTTP_DEPENDENCY");
    if (hasPath(imports, (parts) => includesSequence(parts, ["crate", "socket_relay"]))
      || /\bSocket(?:Connection|Context|Relay)[A-Za-z0-9_]*\b/u.test(inspected)) failures.push("SHARED_CAPABILITY_SOCKET_DEPENDENCY");
    if (hasPath(imports, (parts) => includesSequence(parts, ["tokio", "net"])
      || includesSequence(parts, ["std", "net"]))
      || /\bTcp(?:Listener|Socket|Stream)\b/u.test(inspected)) failures.push("SHARED_CAPABILITY_CONCRETE_TRANSPORT");
    if (hasPath(imports, (parts) => ["openssl", "rustls", "tokio_openssl", "tokio_rustls"].includes(parts[0])
      || includesSegment(parts, "tls"))
      || /\bTls[A-Za-z0-9_]*\b/u.test(inspected)) failures.push("SHARED_CAPABILITY_TLS_DEPENDENCY");
  }
  if (role === "observation_application") {
    if (hasPath(imports, (parts) => parts[0] === "intercept_proxy_infrastructure")) failures.push("OBSERVATION_APPLICATION_INFRASTRUCTURE");
    if (hasPath(imports, (parts) => parts[0] === "tauri")) failures.push("OBSERVATION_APPLICATION_TAURI");
    if (hasPath(imports, (parts) => parts[0] === "tracing")) failures.push("OBSERVATION_APPLICATION_TRACING");
    if (hasPath(imports, (parts) => ["rusqlite", "sqlx"].includes(parts[0]) || includesSegment(parts, "sqlite"))) failures.push("OBSERVATION_APPLICATION_PERSISTENCE");
    if (/\bEventHub\b/u.test(inspected)) failures.push("OBSERVATION_APPLICATION_EVENT_HUB");
  }
  if (role === "observation_store") {
    if (hasPath(imports, (parts) => parts[0] === "tauri")) failures.push("OBSERVATION_STORE_TAURI");
    if (hasPath(imports, (parts) => parts[0] === "tracing")) failures.push("OBSERVATION_STORE_TRACING");
    if (hasPath(imports, (parts) => ["rusqlite", "sqlx"].includes(parts[0]) || includesSegment(parts, "sqlite"))) failures.push("OBSERVATION_STORE_PERSISTENCE");
    if (/\bEventHub\b/u.test(inspected)) failures.push("OBSERVATION_STORE_EVENT_HUB");
  }
  if (role === "observation_runtime"
    && hasPath(imports, (parts) => ["rusqlite", "sqlx"].includes(parts[0]) || includesSegment(parts, "sqlite"))) {
    failures.push("OBSERVATION_RUNTIME_PERSISTENCE");
  }
  if (role === "observation_sqlite"
    && /\b(?:ExchangeObservation(?:Event|Page|Query|Record|Store)?|ExchangeContext)\b/u.test(inspected)) {
    failures.push("OBSERVATION_SQLITE_PAYLOAD");
  }
  return [...new Set(failures)].sort();
}

function productionSuppressionCodes(source) {
  const inspected = productionRustSource(source).replace(
    /#!?\s*\[\s*cfg_attr\s*\(\s*not\s*\(\s*target_os\s*=\s*[^)]*\)\s*,\s*allow\s*\(\s*dead_code\s*\)\s*\)\s*\]/gu,
    "",
  );
  for (const match of inspected.matchAll(/\ballow\s*\(([^)]*)\)/gu)) {
    const lints = match[1].split(",").map((lint) => lint.trim());
    if (lints.some((lint) => lint === "dead_code" || lint === "unused" || /^unused_[a-z_]+$/u.test(lint))) {
      return ["PRODUCTION_UNUSED_SUPPRESSION"];
    }
  }
  return [];
}

function frontendViolationCodes(source) {
  const inspected = source.replace(/\/\*[\s\S]*?\*\//gu, " ").replace(/\/\/.*$/gmu, " ");
  const failures = [];
  if (/(?:from\s+|import\s*\(|require\s*\()\s*["'](?:semver|compare-versions)["']/u.test(inspected)) {
    failures.push("FRONTEND_SEMVER_ENGINE");
  }
  const semverBehavior = /\.split\(\s*["']\.["']\s*\)[\s\S]{0,180}(?:\.map\(\s*Number|parseInt\s*\(|Number\s*\()/u;
  const semverPattern = /\/[^/]*(?:\\d|\[0-9\])\+?\\\.[^/]*(?:\\d|\[0-9\])\+?\\\.[^/]*(?:\\d|\[0-9\])[^/]*\/[gimsuy]*[\s\S]{0,100}\.test\(/u;
  if (semverBehavior.test(inspected) || semverPattern.test(inspected)) failures.push("FRONTEND_SEMVER_VALIDATION_COPY");

  const identifierPattern = /\/[^/]*\[[^\]]*a-z[^\]]*\][^/]*\[[^\]]*a-z[^\]]*0-9[^\]]*-[^\]]*\][^/]*\/[gimsuy]*[\s\S]{0,120}\.test\(/iu;
  if (identifierPattern.test(inspected)) failures.push("FRONTEND_PACKAGE_ID_VALIDATION_COPY");

  const planeSet = /new\s+Set\s*\(\s*\[(?=[^\]]*["']http["'])(?=[^\]]*["']socket["'])[^\]]*\]\s*\)[\s\S]{0,120}\.has\(/u;
  if (planeSet.test(inspected)) failures.push("FRONTEND_DATA_PLANE_VALIDATION_COPY");
  return failures.sort();
}

function frontendObservationViolationCodes(source) {
  const inspected = source.replace(/\/\*[\s\S]*?\*\//gu, " ").replace(/\/\/.*$/gmu, " ");
  return /(?:from\s+|import\s*\()\s*["']@tauri-apps\/api\/event["']/u.test(inspected)
    ? ["OBSERVATION_FRONTEND_DIRECT_TAURI_EVENT"]
    : [];
}

function roleForProxyFile(relative) {
  if (/^(?:http|forward|reverse|message)(?:\.rs|\/)/u.test(relative)) return "http";
  if (/^socket_relay(?:\.rs|\/)/u.test(relative)) return "socket";
  if (/^(?:listener|transport)(?:\.rs|\/)/u.test(relative)) return "neutral";
  return undefined;
}

const fixtureCases = [
  ["HTTP grouped alias import", "http", "use crate::{socket_relay::{SocketRelayService as Relay, config::SocketRelayConfig as C}, tls::TlsEvidence};", ["HTTP_SOCKET_IMPORT"]],
  ["HTTP grouped aliased domain contract", "http", "use intercept_proxy_domain::{workspace::{SocketTopology as Topology}, WorkspaceId};", ["HTTP_SOCKET_CONTRACT_IMPORT"]],
  ["HTTP fully qualified Socket path", "http", "fn build() { crate::socket_relay::SocketRelayService::build(); }", ["HTTP_SOCKET_IMPORT"]],
  ["Socket nested grouped Hyper alias", "socket", "use hyper::{Request as WireRequest, header::{HeaderMap as Headers}};", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["Socket aliased HTTP rule module", "socket", "use intercept_proxy_domain::{rule::{Rule as AnyName}, socket_document_rule::SocketDocumentRuleDefinition};", ["SOCKET_HTTP_RULE_IMPORT"]],
  ["Socket document rule is allowed", "socket", "use intercept_proxy_domain::socket_document_rule::{SocketDocumentRuleDefinition as Rule, SocketRuleStage};", []],
  ["neutral grouped upward imports", "neutral", "use {intercept_proxy_application::{facade::TrafficFacade as F}, tauri::{Manager as M}};", ["NEUTRAL_APPLICATION_UPWARD", "NEUTRAL_UI_UPWARD"]],
  ["neutral fully qualified UI path", "neutral", "fn attach() { tauri::Manager::manage(app, state); }", ["NEUTRAL_UI_UPWARD"]],
  ["Document grouped domain dependencies", "domain_document", "use crate::{protocol_package::PackageId, external_package::ExternalDocumentWire, protocol_document_rule::ProtocolDocumentRule};", ["DOCUMENT_EXTERNAL_PACKAGE_DEPENDENCY", "DOCUMENT_PROTOCOL_PACKAGE_DEPENDENCY", "DOCUMENT_RULE_DEPENDENCY"]],
  ["Document upward and Rhai dependencies", "domain_document", "use {intercept_proxy_application::TrafficFacade, intercept_proxy_infrastructure::Store, tauri::Manager}; fn engine() { rhai::Engine::new(); }", ["DOCUMENT_APPLICATION_UPWARD", "DOCUMENT_INFRASTRUCTURE_UPWARD", "DOCUMENT_RHAI_DEPENDENCY", "DOCUMENT_TAURI_UPWARD"]],
  ["Document protocol-neutral dependencies are allowed", "domain_document", "use crate::{error::DomainError, id::WorkspaceId};", []],
  ["protocol package cannot define Document types", "domain_protocol_package", "pub struct Document;", ["PROTOCOL_PACKAGE_DOCUMENT_TYPE"]],
  ["protocol package cannot re-export Document types", "domain_protocol_package", "pub use crate::document::{DocumentSchema as Schema};", ["PROTOCOL_PACKAGE_DOCUMENT_TYPE"]],
  ["Exchange upward and persistence dependencies", "exchange", "use {intercept_proxy_application::TrafficFacade, intercept_proxy_infrastructure::ObservationStore, tauri::Manager, rusqlite::Connection}; struct Runtime { store: Store, events: EventHub }", ["EXCHANGE_APPLICATION_UPWARD", "EXCHANGE_EVENT_HUB_DEPENDENCY", "EXCHANGE_INFRASTRUCTURE_UPWARD", "EXCHANGE_SQLITE_DEPENDENCY", "EXCHANGE_STORE_DEPENDENCY", "EXCHANGE_TAURI_UPWARD"]],
  ["Exchange domain dependency is allowed", "exchange", "use intercept_proxy_domain::{Document, DomainError};", []],
  ["Domain Document rules reject runtime transport identities", "domain_document_rules", "use {intercept_proxy_runtime::{HttpConnectionIdentity, SocketConnectionIdentity}, intercept_proxy_exchange::HttpContext};", ["DOCUMENT_RULE_EXCHANGE_DEPENDENCY", "DOCUMENT_RULE_RUNTIME_DEPENDENCY", "DOCUMENT_RULE_TRANSPORT_TYPE"]],
  ["Domain Document rules allow pure domain contracts", "domain_document_rules", "use crate::{Document, ListenerId, ProtocolPackageRef};", []],
  ["HTTP Document rules reject Socket runtime types", "infra_http_document_rules", "use intercept_proxy_runtime::{HttpConnectionIdentity, SocketConnectionIdentity};", ["HTTP_DOCUMENT_RULE_SOCKET_DEPENDENCY"]],
  ["HTTP Document rules allow HTTP runtime types", "infra_http_document_rules", "use intercept_proxy_runtime::{HttpConnectionIdentity, HttpDirectionCapabilities};", []],
  ["Socket Document rules reject HTTP runtime types", "infra_socket_document_rules", "use intercept_proxy_runtime::{HttpConnectionIdentity, SocketConnectionIdentity};", ["SOCKET_DOCUMENT_RULE_HTTP_DEPENDENCY"]],
  ["Socket Document rules allow Socket runtime types", "infra_socket_document_rules", "use intercept_proxy_runtime::SocketConnectionIdentity;", []],
  ["Capability stage traits cannot be redefined outside Exchange", "capability_non_owner", "pub trait Decode<P, D> {} pub trait Rules {}", ["CAPABILITY_STAGE_TRAIT_OUTSIDE_EXCHANGE"]],
  ["Capability stage trait implementations are allowed outside Exchange", "capability_non_owner", "impl Decode<Http, Upstream> for HttpDecode {} impl Rules for HttpRules {}", []],
  ["Shared capability helper rejects protocol and concrete transport imports", "shared_capability_helper", "use {hyper::Request, tokio::net::TcpStream, crate::socket_relay::SocketRelayService, rustls::ClientConfig};", ["SHARED_CAPABILITY_CONCRETE_TRANSPORT", "SHARED_CAPABILITY_HTTP_DEPENDENCY", "SHARED_CAPABILITY_SOCKET_DEPENDENCY", "SHARED_CAPABILITY_TLS_DEPENDENCY"]],
  ["Shared capability helper allows protocol-neutral async IO", "shared_capability_helper", "use {tokio::io::{AsyncRead, AsyncWrite}, tokio_util::sync::CancellationToken};", []],
  ["Application observation DTO rejects runtime and persistence ownership", "observation_application", "use {intercept_proxy_infrastructure::ExchangeObservationStore, tauri::Manager, tracing::Event, rusqlite::Connection}; struct ObservationDto { events: EventHub }", ["OBSERVATION_APPLICATION_EVENT_HUB", "OBSERVATION_APPLICATION_INFRASTRUCTURE", "OBSERVATION_APPLICATION_PERSISTENCE", "OBSERVATION_APPLICATION_TAURI", "OBSERVATION_APPLICATION_TRACING"]],
  ["Infrastructure observation store rejects runtime and persistence ownership", "observation_store", "use {tauri::Manager, tracing::Event, rusqlite::Connection}; struct ObservationStore { events: EventHub }", ["OBSERVATION_STORE_EVENT_HUB", "OBSERVATION_STORE_PERSISTENCE", "OBSERVATION_STORE_TAURI", "OBSERVATION_STORE_TRACING"]],
  ["Tauri observation layer rejects direct persistence", "observation_runtime", "use rusqlite::Connection;", ["OBSERVATION_RUNTIME_PERSISTENCE"]],
  ["SQLite cannot own Exchange payload observations", "observation_sqlite", "struct Row { record: ExchangeObservationRecord, context: ExchangeContext }", ["OBSERVATION_SQLITE_PAYLOAD"]],
  ["Capture observation UI uses the shared app event subscription", "frontend_observation", "import { listen } from '@tauri-apps/api/event';", ["OBSERVATION_FRONTEND_DIRECT_TAURI_EVENT"]],
  ["test-only forbidden Rust import is ignored", "socket", "use tokio::net::TcpStream; #[cfg(test)] mod tests { use hyper::{Request as Hidden}; }", []],
  ["test-only Document boundary violation is ignored", "domain_document", "#[cfg(test)] mod tests { use crate::external_package::ExternalDocumentWire; }", []],
  ["test-only protocol package Document type is ignored", "domain_protocol_package", "#[cfg(test)] struct Document;", []],
  ["test-only Exchange persistence is ignored", "exchange", "#[cfg(test)] struct Store;", []],
  ["cfg test array return semicolon is ignored", "socket", "#[cfg(test)] fn hidden() -> [u8; 1] { let _ = hyper::Request::new(()); [0] }", []],
  ["cfg test const array return is ignored", "socket", "#[cfg(test)] fn hidden() -> [u8; { 1 }] { let _ = hyper::Request::new(()); [0] }", []],
  ["cfg test less-than const stops before production", "socket", "#[cfg(test)] const LESS: bool = 1 < 2;\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test less-than static stops before production", "socket", "#[cfg(test)] static LESS: bool = 1 < 2;\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test const-generic unit struct stops before production", "socket", "#[cfg(test)] struct Hidden<const B: bool = { 1 < 2 }>;\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test extern ABI fn stops before production", "socket", "#[cfg(test)] extern \"C\" fn hidden(_: crate::Arg) {}\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test unsafe extern block stops before production", "socket", "#[cfg(test)] unsafe extern \"C\" { fn hidden(_: crate::Arg); }\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test const unsafe fn stops before production", "socket", "#[cfg(test)] const unsafe fn hidden() {}\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test const extern ABI fn stops before production", "socket", "#[cfg(test)] const extern \"C\" fn hidden() {}\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test macro_rules stops before production", "socket", "#[cfg(test)] macro_rules! hidden { () => { hyper::Request::new(()) }; }\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test braced item macro stops before production", "socket", "#[cfg(test)] hidden! { hyper::Request }\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["cfg test return type macro is ignored", "socket", "#[cfg(test)] fn hidden() -> ty!{} { let _ = hyper::Request::new(()); }\nuse tokio::net::TcpStream;", []],
  ["comment cannot forge cfg test masking", "socket", "// #[cfg(test)]\nuse hyper::Request;", ["SOCKET_HTTP_DTO_IMPORT"]],
  ["Rust strings and comments are ignored", "neutral", "const NOTE: &str = \"tauri::Manager\"; // intercept_proxy_application::facade", []],
  ["frontend renamed package validator", "frontend", "const acceptable = (value) => /^[a-z][a-z0-9-]+$/.test(value);", ["FRONTEND_PACKAGE_ID_VALIDATION_COPY"]],
  ["frontend renamed SemVer validator", "frontend", "const acceptable = (value) => value.split('.').map(Number).every(Number.isInteger);", ["FRONTEND_SEMVER_VALIDATION_COPY"]],
  ["frontend data-plane constant copy", "frontend", "const acceptable = (value) => new Set(['http', 'socket']).has(value);", ["FRONTEND_DATA_PLANE_VALIDATION_COPY"]],
];

const productionSuppressionFixtures = [
  ["production dead-code suppression", "#[allow(dead_code)] fn obsolete() {}", ["PRODUCTION_UNUSED_SUPPRESSION"]],
  ["production unused import suppression", "#![allow(unused_imports)]\nuse crate::obsolete;", ["PRODUCTION_UNUSED_SUPPRESSION"]],
  ["conditional production suppression", "#[cfg_attr(feature = \"old\", allow(dead_code))] fn obsolete() {}", ["PRODUCTION_UNUSED_SUPPRESSION"]],
  ["target-specific implementation suppression is allowed", "#![cfg_attr(not(target_os = \"android\"), allow(dead_code))]", []],
  ["namespaced clippy lint is not a Rust unused suppression", "#[allow(clippy::unused_async)] async fn required_by_trait() {}", []],
  ["test-only suppression is allowed", "#[cfg(test)] #[allow(dead_code)] fn fixture_helper() {}", []],
];

const roleFixtures = [
  ["http.rs", "http"], ["forward.rs", "http"], ["reverse.rs", "http"], ["message.rs", "http"],
  ["http/contracts.rs", "http"], ["socket_relay.rs", "socket"], ["listener.rs", "neutral"], ["transport.rs", "neutral"],
];

function runFixtures() {
  const failures = [];
  for (const [name, role, source, expected] of fixtureCases) {
    const actual = role === "frontend"
      ? frontendViolationCodes(source)
      : role === "frontend_observation"
        ? frontendObservationViolationCodes(source)
        : rustViolationCodes(role, source);
    if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) failures.push(`${name}: expected [${expected}], got [${actual}]`);
  }
  for (const [name, source, expected] of productionSuppressionFixtures) {
    const actual = productionSuppressionCodes(source);
    if (JSON.stringify(actual) !== JSON.stringify(expected)) failures.push(`${name}: expected [${expected}], got [${actual}]`);
  }
  for (const [file, expected] of roleFixtures) {
    const actual = roleForProxyFile(file);
    if (actual !== expected) failures.push(`role scope ${file}: expected ${expected}, got ${actual}`);
    const probe = expected === "http"
      ? ["use crate::{socket_relay::{SocketRelayService as Hidden}};", "HTTP_SOCKET_IMPORT"]
      : expected === "socket"
        ? ["use hyper::{Request as Hidden};", "SOCKET_HTTP_DTO_IMPORT"]
        : ["use intercept_proxy_application::{facade::TrafficFacade as Hidden};", "NEUTRAL_APPLICATION_UPWARD"];
    if (!rustViolationCodes(expected, probe[0]).includes(probe[1])) failures.push(`role scope ${file}: forbidden probe was not scanned`);
  }
  return failures;
}

async function filesBelow(directory, extensions) {
  let entries;
  try {
    entries = await readdir(directory, { withFileTypes: true });
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
  const files = [];
  for (const entry of entries) {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await filesBelow(absolute, extensions)));
    else if (entry.isFile() && extensions.some((extension) => entry.name.endsWith(extension))) files.push(absolute);
  }
  return files;
}

function isProduction(file) {
  return !/(?:^|\/)(?:tests?|__tests__)(?:\/|\.|$)/u.test(file)
    && !/(?:^|\/)[^/]+_tests\.rs$/u.test(file)
    && !/\.(?:test|spec|test-support)\.(?:ts|tsx)$/u.test(file);
}

async function scanProduction() {
  const failures = [];
  for (const [absolute, role] of [
    [applicationObservationFile, "observation_application"],
    [infrastructureObservationFile, "observation_store"],
    [runtimeObservationFile, "observation_runtime"],
  ]) {
    const source = await readFile(absolute, "utf8");
    for (const code of rustViolationCodes(role, source)) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  const runtimeObservation = productionRustSource(await readFile(runtimeObservationFile, "utf8"));
  for (const [pattern, code] of [
    [/\bimpl\s*<[^>]+>\s+Layer\s*</u, "OBSERVATION_RUNTIME_LAYER_MISSING"],
    [/\bEventHub\b/u, "OBSERVATION_RUNTIME_EVENT_HUB_MISSING"],
    [/\bpublish_changed\s*\(/u, "OBSERVATION_RUNTIME_PUBLICATION_MISSING"],
  ]) {
    if (!pattern.test(runtimeObservation)) failures.push(`${path.relative(repositoryRoot, runtimeObservationFile)}: [${code}]`);
  }
  for (const absolute of await filesBelow(sqliteSourceRoot, [".rs"])) {
    const relative = path.relative(sqliteSourceRoot, absolute).split(path.sep).join("/");
    if (!isProduction(relative)) continue;
    for (const code of rustViolationCodes("observation_sqlite", await readFile(absolute, "utf8"))) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  for (const absolute of await filesBelow(captureSourceRoot, [".ts", ".tsx"])) {
    const relative = path.relative(captureSourceRoot, absolute).split(path.sep).join("/");
    if (!isProduction(relative) || !relative.startsWith("exchange-observation-")) continue;
    for (const code of frontendObservationViolationCodes(await readFile(absolute, "utf8"))) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  for (const absolute of sharedCapabilityHelperFiles) {
    for (const code of rustViolationCodes("shared_capability_helper", await readFile(absolute, "utf8"))) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  for (const sourceRoot of [cratesSourceRoot, tauriAppSourceRoot]) {
    for (const absolute of await filesBelow(sourceRoot, [".rs"])) {
      const relative = path.relative(sourceRoot, absolute).split(path.sep).join("/");
      if (!isProduction(relative)) continue;
      const source = await readFile(absolute, "utf8");
      for (const code of productionSuppressionCodes(source)) {
        failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
      }
    }
  }
  for (const absolute of await filesBelow(cratesSourceRoot, [".rs"])) {
    const relative = path.relative(cratesSourceRoot, absolute).split(path.sep).join("/");
    if (!isProduction(relative)) continue;
    const source = await readFile(absolute, "utf8");
    if (absolute === exchangeCapabilityFile) {
      const inspected = productionRustSource(source);
      for (const traitName of ["Frame", "Decode", "Display", "Rules", "Encode"]) {
        if (!new RegExp(`\\bpub\\s+trait\\s+${traitName}\\b`, "u").test(inspected)) {
          failures.push(`${path.relative(repositoryRoot, absolute)}: [EXCHANGE_CAPABILITY_TRAIT_MISSING:${traitName}]`);
        }
      }
      continue;
    }
    for (const code of rustViolationCodes("capability_non_owner", source)) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  for (const absolute of await filesBelow(proxySourceRoot, [".rs"])) {
    const relative = path.relative(proxySourceRoot, absolute).split(path.sep).join("/");
    const role = roleForProxyFile(relative);
    if (!role || !isProduction(relative)) continue;
    for (const code of rustViolationCodes(role, await readFile(absolute, "utf8"))) failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
  }
  for (const [root, role] of [
    [domainDocumentSourceRoot, "domain_document"],
    [domainProtocolPackageSourceRoot, "domain_protocol_package"],
    [exchangeSourceRoot, "exchange"],
  ]) {
    for (const absolute of await filesBelow(root, [".rs"])) {
      const relative = path.relative(root, absolute).split(path.sep).join("/");
      if (!isProduction(relative)) continue;
      for (const code of rustViolationCodes(role, await readFile(absolute, "utf8"))) {
        failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
      }
    }
  }
  for (const absolute of await filesBelow(domainSourceRoot, [".rs"])) {
    const relative = path.relative(domainSourceRoot, absolute).split(path.sep).join("/");
    if (!isProduction(relative)
      || (relative !== "protocol_document_rule.rs" && !relative.startsWith("protocol_document_rule/"))) continue;
    for (const code of rustViolationCodes("domain_document_rules", await readFile(absolute, "utf8"))) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  for (const absolute of await filesBelow(listenerRuntimeSourceRoot, [".rs"])) {
    const relative = path.relative(listenerRuntimeSourceRoot, absolute).split(path.sep).join("/");
    if (!isProduction(relative)) continue;
    const role = relative === "http_protocol_pipeline.rs" || relative.startsWith("http_protocol_pipeline/")
      ? "infra_http_document_rules"
      : relative === "document_rules.rs" || relative.startsWith("document_rules/")
        ? "infra_socket_document_rules"
        : undefined;
    if (!role) continue;
    for (const code of rustViolationCodes(role, await readFile(absolute, "utf8"))) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  for (const absolute of await filesBelow(path.join(repositoryRoot, "src"), [".ts", ".tsx"])) {
    const relative = path.relative(repositoryRoot, absolute).split(path.sep).join("/");
    if (!isProduction(relative) || relative === "src/generated/rust-types.ts") continue;
    for (const code of frontendViolationCodes(await readFile(absolute, "utf8"))) failures.push(`${relative}: [${code}]`);
  }
  return failures;
}

const failures = [...runFixtures(), ...(await scanProduction())];
if (failures.length > 0) {
  console.error("Architecture boundary gate failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exitCode = 1;
} else {
  console.log(`Architecture boundary fixtures passed (${fixtureCases.length} behavior, ${roleFixtures.length} role cases).`);
  console.log("Architecture production boundary gate passed.");
}
