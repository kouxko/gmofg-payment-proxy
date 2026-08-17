import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { productionRustSource } from "./rust-lexical-scan.mjs";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const socketRoot = path.join(
  repositoryRoot,
  "src-tauri/crates/proxy/src/socket_relay",
);

const forbidden = [
  ["SOCKET_HYPER", /\b(?:hyper|hyper_util)::|\bHyper[A-Za-z0-9_]*\b/u],
  ["SOCKET_PIPELINE", /\bPipelinePorts\b/u],
  [
    "SOCKET_HTTP_MESSAGE",
    /\bhttp::|\b(?:crate|super)::(?:[A-Za-z_][A-Za-z0-9_]*::)*message\b|\bHttpMessage\b/u,
  ],
  [
    "SOCKET_CAPTURE_SESSION",
    /\b(?:Capture|CapturedRequest|CapturedResponse|CaptureSession|SessionRecord|SessionStore)\b/u,
  ],
  [
    "SOCKET_BREAKPOINT",
    /\b(?:Breakpoint|BreakpointDecision)\b/u,
  ],
  ["SOCKET_MITM", /\b(?:Mitm|MITM)[A-Za-z0-9_]*\b|\bmitm::/u],
  [
    "SOCKET_BODY_CODEC",
    /\b(?:BodyCodec|ContentEncoding)\b|\bencoding_rs::/u,
  ],
];

const fixtures = [
  ["tokio transport is allowed", "use tokio::net::TcpStream;", []],
  ["Socket Rule stage is allowed", "let stage = SocketRelayStage::Rule;", []],
  ["test-only Hyper is ignored", "#[cfg(test)] mod tests { use hyper::Request; }", []],
  ["cfg test array return semicolon is ignored", "#[cfg(test)] fn hidden() -> [u8; 1] { let _ = hyper::Request::new(()); [0] }", []],
  ["cfg test const array return is ignored", "#[cfg(test)] fn hidden() -> [u8; { 1 }] { let _ = hyper::Request::new(()); [0] }", []],
  ["cfg test less-than const stops before production", "#[cfg(test)] const LESS: bool = 1 < 2;\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test less-than static stops before production", "#[cfg(test)] static LESS: bool = 1 < 2;\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test const-generic unit struct stops before production", "#[cfg(test)] struct Hidden<const B: bool = { 1 < 2 }>;\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test extern ABI fn stops before production", "#[cfg(test)] extern \"C\" fn hidden(_: crate::Arg) {}\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test unsafe extern block stops before production", "#[cfg(test)] unsafe extern \"C\" { fn hidden(_: crate::Arg); }\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test const unsafe fn stops before production", "#[cfg(test)] const unsafe fn hidden() {}\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test const extern ABI fn stops before production", "#[cfg(test)] const extern \"C\" fn hidden() {}\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test macro_rules stops before production", "#[cfg(test)] macro_rules! hidden { () => { hyper::Request::new(()) }; }\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test braced item macro stops before production", "#[cfg(test)] hidden! { hyper::Request }\nuse hyper::Request;", ["SOCKET_HYPER"]],
  ["cfg test return type macro is ignored", "#[cfg(test)] fn hidden() -> ty!{} { let _ = hyper::Request::new(()); }\nuse tokio::net::TcpStream;", []],
  ["comment cannot forge cfg test masking", "// #[cfg(test)]\nuse hyper::Request;", ["SOCKET_HYPER"]],
  [
    "neutral transport and TLS are allowed",
    "use crate::{tls::TlsEvidence, transport::relay::RelayBytes};",
    [],
  ],
  ["Hyper is rejected", "use hyper::server::conn::http1;", ["SOCKET_HYPER"]],
  [
    "pipeline is rejected",
    "use crate::transport::PipelinePorts;",
    ["SOCKET_PIPELINE"],
  ],
  [
    "HTTP message is rejected",
    "use crate::message::Message;",
    ["SOCKET_HTTP_MESSAGE"],
  ],
  [
    "capture and sessions are rejected",
    "let _: CaptureSession; let _: SessionStore;",
    ["SOCKET_CAPTURE_SESSION"],
  ],
  [
    "breakpoints are rejected",
    "let _: BreakpointDecision;",
    ["SOCKET_BREAKPOINT"],
  ],
  ["MITM is rejected", "use crate::forward::MitmRuntime;", ["SOCKET_MITM"]],
  [
    "body codecs are rejected",
    "let _: BodyCodec;",
    ["SOCKET_BODY_CODEC"],
  ],
];

function violations(source) {
  const inspected = productionRustSource(source);
  return forbidden
    .filter(([, pattern]) => pattern.test(inspected))
    .map(([code]) => code);
}

function runFixtures() {
  const failures = [];
  for (const [name, source, expected] of fixtures) {
    const actual = violations(source).sort();
    const wanted = [...expected].sort();
    if (JSON.stringify(actual) !== JSON.stringify(wanted)) {
      failures.push(`${name}: expected ${wanted.join(",")}, got ${actual.join(",")}`);
    }
  }
  return failures;
}

async function rustFiles(directory) {
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
    if (entry.isDirectory()) files.push(...(await rustFiles(absolute)));
    else if (entry.isFile() && entry.name.endsWith(".rs")) files.push(absolute);
  }
  return files;
}

async function scanProduction() {
  const failures = [];
  for (const absolute of await rustFiles(socketRoot)) {
    if (/(?:^|\/)(?:tests?|[^/]+_tests)(?:\/|\.rs$)/u.test(absolute)) continue;
    const source = await readFile(absolute, "utf8");
    for (const code of violations(source)) {
      failures.push(`${path.relative(repositoryRoot, absolute)}: [${code}]`);
    }
  }
  return failures;
}

const failures = [...runFixtures(), ...(await scanProduction())];
if (failures.length > 0) {
  console.error("Socket Relay boundary gate failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exitCode = 1;
} else {
  console.log(`Socket Relay boundary fixtures passed (${fixtures.length} cases).`);
  console.log("Socket Relay production boundary gate passed.");
}
