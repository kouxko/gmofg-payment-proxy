import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

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
  ["SOCKET_RULE", /\b(?:Rule|RuleSet|RuleAction|RuleEngine)\b/u],
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
  ["rules are rejected", "let _: RuleEngine;", ["SOCKET_RULE"]],
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

function stripComments(source) {
  return source
    .replace(/\/\*[\s\S]*?\*\//gu, " ")
    .replace(/\/\/.*$/gmu, " ");
}

function violations(source) {
  const inspected = stripComments(source);
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
    if (/(?:^|\/)tests?(?:\/|\.rs$)/u.test(absolute)) continue;
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
