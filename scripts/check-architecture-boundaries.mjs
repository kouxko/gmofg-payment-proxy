import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { productionRustSource } from "./rust-lexical-scan.mjs";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const proxySourceRoot = path.join(repositoryRoot, "src-tauri/crates/proxy/src");

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
  const qualified = [...source.matchAll(/\b(?:crate|super|self|tauri|hyper|hyper_util|http|http_body_util|intercept_proxy_application|intercept_proxy_domain)(?:::[A-Za-z_][A-Za-z0-9_]*)+/gu)]
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
  const imports = normalizedRustPaths(productionRustSource(source));
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
  return [...new Set(failures)].sort();
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
  ["test-only forbidden Rust import is ignored", "socket", "use tokio::net::TcpStream; #[cfg(test)] mod tests { use hyper::{Request as Hidden}; }", []],
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
  ["frontend response-shape guard is allowed", "frontend", "function validateSocketCapturePage(value) { return typeof value === 'object'; }", []],
];

const roleFixtures = [
  ["http.rs", "http"], ["forward.rs", "http"], ["reverse.rs", "http"], ["message.rs", "http"],
  ["http/contracts.rs", "http"], ["socket_relay.rs", "socket"], ["listener.rs", "neutral"], ["transport.rs", "neutral"],
];

function runFixtures() {
  const failures = [];
  for (const [name, role, source, expected] of fixtureCases) {
    const actual = role === "frontend" ? frontendViolationCodes(source) : rustViolationCodes(role, source);
    if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) failures.push(`${name}: expected [${expected}], got [${actual}]`);
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
  for (const absolute of await filesBelow(proxySourceRoot, [".rs"])) {
    const relative = path.relative(proxySourceRoot, absolute).split(path.sep).join("/");
    const role = roleForProxyFile(relative);
    if (!role || !isProduction(relative)) continue;
    for (const code of rustViolationCodes(role, await readFile(absolute, "utf8"))) failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
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
