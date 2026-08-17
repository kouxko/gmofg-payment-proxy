import {
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { checkCrateDependencies } from "./runtime-crate-dependencies.mjs";
import { productionRustSource, productionRustWithStrings } from "./rust-lexical-scan.mjs";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const runtimeCrate = "src-tauri/crates/proxy";
const requireZeroDebt =
  process.argv.includes("--require-zero-debt") ||
  process.env.RUNTIME_ARCH_REQUIRE_ZERO_DEBT === "1";

// These entries describe owned facilities, not exceptions to the handler rule. A removed or
// renamed site makes the ledger stale and fails the gate until this list is updated deliberately.
const productionSpawnLedger = [
  ledger("src/supervisor/facade.rs", "start", "tokio::spawn", "listener-supervisor", "core.run_start", "src/supervisor/tests/lifecycle.rs", "starts_listens_and_stops_three_channels"),
  ledger("src/supervisor/facade.rs", "stop", "tokio::spawn", "listener-supervisor", "core.run_stop", "src/supervisor/tests/lifecycle.rs", "starts_listens_and_stops_three_channels"),
  ledger("src/supervisor/facade.rs", "restart", "tokio::spawn", "listener-supervisor", "core.run_restart", "src/supervisor/tests/lifecycle.rs", "starts_listens_and_stops_three_channels"),
  ledger("src/supervisor/tasks.rs", "spawn_listener_task", "tokio::spawn", "listener-supervisor", "prepared.service.run_listener", "src/supervisor/tests/shutdown.rs", "listener_panic_faults_epoch_and_cancels_sibling"),
  ledger("src/supervisor/tasks.rs", "spawn_watchdog", "tokio::spawn", "listener-supervisor", "fatal_rx.recv", "src/supervisor/tests/shutdown.rs", "listener_panic_faults_epoch_and_cancels_sibling"),
  ledger("src/http/tracking.rs", "spawn", "tokio::spawn", "lifecycle-facility", "upstream HTTP/1 connection ended", "src/http/tests.rs", "downstream_response_write_stops_when_supervisor_cancels"),
  ledger("src/listener/task_scope.rs", "spawn_owned", "TaskTracker::spawn", "connection-task-scope", "task", "src/listener/task_scope/tests.rs", "close_spawn_barrier_accepts_and_drains_or_rejects_without_polling"),
  ledger("src/listener/supervisor.rs", "run_bound", "JoinSet::spawn", "listener-supervisor", "run_connection", "src/listener/supervisor/tests.rs", "child_panic_faults_listener_and_cancels_sibling"),
  ledger("src/listener/supervisor.rs", "run_bound", "JoinSet::spawn", "listener-supervisor", "handler.reject", "src/listener/supervisor/tests.rs", "cidr_rejection_emits_no_admission_or_terminal_event"),
];

// Phase-1 migration debt is intentionally separate from the permanent ownership ledger. Delete
// one row as soon as its future moves behind ConnectionTaskScope::spawn_owned. The zero-debt gate
// (`--require-zero-debt` or RUNTIME_ARCH_REQUIRE_ZERO_DEBT=1) turns every remaining row into a
// failure, so these sites cannot become a permanent allow-list.
const productionSpawnDebt = [];

// These are the pre-Phase-2 HTTP transport implementation files. They are not the neutral
// transport kernel and are enumerated so a newly added transport file is neutral by default.
// Phase 2 removes these rows as HTTP code moves under src/http and neutral primitives replace it.
const phase2LegacyHttpTransportDebt = new Set();

function ledger(file, symbol, api, owner, anchor, proofFile, proofAnchor) {
  return { file, symbol, api, owner, anchor, proofFile, proofAnchor };
}

function debt(file, symbol, anchor) {
  return {
    file,
    symbol,
    api: "tokio::spawn",
    owner: "phase1-handler-debt",
    anchor,
    clearBy: "Move the future to ConnectionTaskScope::spawn_owned, then delete this row.",
  };
}

function normalized(relativePath) {
  return relativePath.split(path.sep).join("/");
}

async function filesBelow(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await filesBelow(absolute)));
    else if (entry.isFile()) files.push(absolute);
  }
  return files;
}

const httpDependency = /\b(?:hyper|hyper_util|http|http_body_util)::|\b(?:PipelinePorts|Message|RawHttp1|HttpTarget|ForwardProxy|ReverseProxy|Mitm|MITM)\b/u;
const socketDependency = /\b(?:crate|super|self)::(?:[A-Za-z_][A-Za-z0-9_]*::)*socket_relay\b|\bSocketRelay[A-Za-z0-9_]*\b/u;

const socketForbidden = [
  ["SOCKET_HYPER", /\b(?:hyper|hyper_util)::|\bHyper[A-Za-z0-9_]*\b/u, "Hyper"],
  ["SOCKET_PIPELINE", /\bPipelinePorts\b/u, "PipelinePorts"],
  ["SOCKET_HTTP_MESSAGE", /\bhttp::|\b(?:crate|super)::(?:[A-Za-z_][A-Za-z0-9_]*::)*message\b|\bHttpMessage\b/u, "HTTP Message"],
  ["SOCKET_CAPTURE", /\b(?:Capture|CapturedRequest|CapturedResponse|CaptureSession|SessionRecord|SessionStore)\b/u, "capture/session"],
  ["SOCKET_BREAKPOINTS", /\b(?:Breakpoint|BreakpointDecision)\b/u, "breakpoints"],
  ["SOCKET_MITM", /\b(?:Mitm|MITM)\b|\bmitm::/u, "MITM"],
  ["SOCKET_BODY_CODEC", /\b(?:BodyCodec|ContentEncoding)\b|\bencoding_rs::/u, "HTTP body codecs"],
];

function sourceRole(file) {
  if (/^src\/listener(?:\.rs|\/)/u.test(file)) return "neutral-listener";
  if (phase2LegacyHttpTransportDebt.has(file)) return "legacy-http-transport";
  if (/^src\/transport(?:\.rs|\/)/u.test(file)) return "neutral-transport";
  if (/^src\/(?:http|forward)(?:\.rs|\/)/u.test(file)) return "http";
  if (/^src\/socket_relay(?:\.rs|\/)/u.test(file)) return "socket";
  return "other";
}

async function runtimeSources(root) {
  const crateRoot = path.join(root, runtimeCrate);
  const sourceRoot = path.join(crateRoot, "src");
  let sources;
  try {
    sources = await filesBelow(sourceRoot);
  } catch {
    return [];
  }
  return Promise.all(
    sources
      .filter((file) => file.endsWith(".rs"))
      .filter((file) => !/(?:^|\/)(?:tests?|[^/]+_tests)(?:\/|\.rs$)/u.test(normalized(path.relative(sourceRoot, file))))
      .map(async (absolute) => {
        const raw = await readFile(absolute, "utf8");
        return {
          absolute,
          file: normalized(path.relative(crateRoot, absolute)),
          source: productionRustWithStrings(raw),
          inspected: productionRustSource(raw),
        };
      }),
  );
}

async function checkSourceBoundaries(root) {
  const violations = [];
  for (const { file, inspected } of await runtimeSources(root)) {
    const role = sourceRole(file);
    if ((role === "neutral-listener" || role === "neutral-transport") && httpDependency.test(inspected)) {
      violations.push(problem("NEUTRAL_HTTP", file, `${role} imports an HTTP-only dependency`));
    }
    if ((role === "neutral-listener" || role === "neutral-transport") && socketDependency.test(inspected)) {
      violations.push(problem("NEUTRAL_SOCKET", file, `${role} imports Socket Relay`));
    }
    if (role === "http" && socketDependency.test(inspected)) {
      violations.push(problem("HTTP_SOCKET", file, "HTTP code imports or branches on Socket Relay"));
    }
    if (role === "socket") {
      for (const [code, pattern, label] of socketForbidden) {
        if (pattern.test(inspected)) violations.push(problem(code, file, `Socket Relay imports ${label}`));
      }
    }
  }
  return violations;
}

function matchingDelimiter(source, start, open, close) {
  let depth = 0;
  for (let index = start; index < source.length; index += 1) {
    if (source[index] === open) depth += 1;
    else if (source[index] === close && --depth === 0) return index;
  }
  return -1;
}

function enclosingSymbol(source, offset) {
  const prefix = source.slice(0, offset);
  const matches = [...prefix.matchAll(/\b(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^{};]*>)?\s*\(/gu)];
  return matches.at(-1)?.[1] ?? "<module>";
}

function inferSpawnApi(source, callee) {
  if (callee === "tokio::spawn" || callee === "tokio::task::spawn") return callee;
  if (callee.includes("spawn_blocking")) return "tokio::task::spawn_blocking";
  const receiver = callee.slice(0, -".spawn".length).split(".").at(-1);
  if (receiver === "tracker") return "TaskTracker::spawn";
  if (receiver && new RegExp(`\\b${receiver}\\s*=\\s*(?:JoinSet(?:::[A-Za-z_][A-Za-z0-9_]*)?|JoinSet::<[^>]+>)::?new\\s*\\(`, "u").test(source)) return "JoinSet::spawn";
  if (receiver && new RegExp(`\\b${receiver}\\s*=\\s*(?:TaskTracker(?:::[A-Za-z_][A-Za-z0-9_]*)?)::?new\\s*\\(`, "u").test(source)) return "TaskTracker::spawn";
  return `${receiver ?? "unknown"}.spawn`;
}

function taskSites(file, source) {
  const sites = [];
  const pattern = /\b(tokio::(?:task::)?spawn(?:_blocking)?|[A-Za-z_][A-Za-z0-9_.]*\.spawn)\s*\(/gu;
  for (const match of source.matchAll(pattern)) {
    const open = source.indexOf("(", match.index);
    const close = matchingDelimiter(source, open, "(", ")");
    const body = close < 0 ? source.slice(open) : source.slice(open, close + 1);
    sites.push({
      file,
      symbol: enclosingSymbol(source, match.index),
      api: inferSpawnApi(source, match[1]),
      body,
      line: source.slice(0, match.index).split(/\r?\n/u).length,
    });
  }
  return sites;
}

function entryMatches(entry, site) {
  const compactBody = site.body.replace(/\s+/gu, "");
  const compactAnchor = entry.anchor.replace(/\s+/gu, "");
  return entry.file === site.file && entry.symbol === site.symbol && entry.api === site.api && compactBody.includes(compactAnchor);
}

function ownerIsValid(entry) {
  if (entry.owner === "listener-supervisor") return /(?:^|\/)supervisor(?:\/|\.rs$)/u.test(entry.file) || /(?:^|\/)listener\/supervisor\.rs$/u.test(entry.file);
  if (entry.owner === "connection-task-scope") return /task_scope\.rs$/u.test(entry.file) && entry.symbol === "spawn_owned";
  if (entry.owner === "lifecycle-facility") return !/(?:^|\/)(?:http|forward|socket_relay)\/.*(?:handler|connect|websocket|http|mitm)\.rs$/u.test(entry.file);
  return false;
}

async function proofIsValid(crateRoot, entry) {
  if (!entry.proofFile || !entry.proofAnchor) return false;
  try {
    return (await readFile(path.join(crateRoot, entry.proofFile), "utf8")).includes(entry.proofAnchor);
  } catch {
    return false;
  }
}

async function checkSpawnLedger(root, ledgerEntries, debtEntries, zeroDebt) {
  const crateRoot = path.join(root, runtimeCrate);
  const sources = await runtimeSources(root);
  const sites = sources.flatMap(({ file, source }) => taskSites(file, source));
  const violations = [];
  const matchedLedger = new Set();
  const matchedDebt = new Set();

  for (const site of sites) {
    const ledgerIndex = ledgerEntries.findIndex((entry) => entryMatches(entry, site));
    if (ledgerIndex >= 0) {
      const entry = ledgerEntries[ledgerIndex];
      matchedLedger.add(ledgerIndex);
      if (!ownerIsValid(entry)) violations.push(problem("SPAWN_OWNER", site.file, `${site.symbol} is not a valid ${entry.owner} owner`, site.line));
      else if (!(await proofIsValid(crateRoot, entry))) violations.push(problem("SPAWN_PROOF", site.file, `${site.symbol} lacks its registered cancellation/join proof`, site.line));
      continue;
    }
    const debtIndex = debtEntries.findIndex((entry) => entryMatches(entry, site));
    if (debtIndex >= 0) {
      matchedDebt.add(debtIndex);
      if (zeroDebt) violations.push(problem("SPAWN_DEBT", site.file, `${site.symbol} still owns direct task creation`, site.line));
      continue;
    }
    violations.push(problem("SPAWN_UNREGISTERED", site.file, `${site.symbol} creates a task with ${site.api} but has no exact owner ledger entry`, site.line));
  }
  ledgerEntries.forEach((entry, index) => {
    if (!matchedLedger.has(index)) violations.push(problem("SPAWN_LEDGER_STALE", entry.file, `${entry.symbol} / ${entry.anchor} is registered but absent`));
  });
  debtEntries.forEach((entry, index) => {
    if (!matchedDebt.has(index)) violations.push(problem("SPAWN_DEBT_STALE", entry.file, `${entry.symbol} debt is gone or changed; delete its debt row`));
  });
  return { violations, sites, activeDebt: debtEntries.filter((_, index) => matchedDebt.has(index)) };
}

function problem(code, file, message, line) {
  return { code, file, line, message };
}

async function scan(root, { ledgerEntries = [], debtEntries = [], zeroDebt = false } = {}) {
  const [crateViolations, boundaryViolations, spawn] = await Promise.all([
    checkCrateDependencies(root),
    checkSourceBoundaries(root),
    checkSpawnLedger(root, ledgerEntries, debtEntries, zeroDebt),
  ]);
  return { ...spawn, violations: [...crateViolations, ...boundaryViolations, ...spawn.violations] };
}

async function materializeFixture(root, fixture) {
  const files = {
    "src-tauri/crates/proxy/Cargo.toml": `[package]\nname = "intercept-proxy-runtime"\nversion = "0.0.0"\n[dependencies]\n`,
    ...fixture.files,
  };
  for (const [relative, contents] of Object.entries(files)) {
    const absolute = path.join(root, relative);
    await mkdir(path.dirname(absolute), { recursive: true });
    await writeFile(absolute, contents);
  }
}

const fixtureCases = [
  {
    name: "clean dependency directions and owned lifecycle spawn pass",
    expected: [],
    files: {
      "src-tauri/crates/domain/Cargo.toml": `[package]\nname = "intercept-proxy-domain"\nversion = "0.0.0"\n[dependencies]\nserde = "1"\n`,
      "src-tauri/crates/application/Cargo.toml": `[package]\nname = "intercept-proxy-application"\nversion = "0.0.0"\n[dependencies]\nintercept-proxy-domain = { path = "../domain" }\n`,
      "src-tauri/crates/proxy/src/listener/supervisor.rs": `pub async fn run() { tokio::spawn(async move { owned().await }); }\n`,
      "src-tauri/crates/proxy/src/listener/tests.rs": `fn cancellation_joins_children() {}\n`,
      "src-tauri/crates/proxy/src/http/handler.rs": `use crate::transport::BoxIo;\n`,
      "src-tauri/crates/proxy/src/socket_relay/handler.rs": `use crate::transport::BoxIo;\nuse crate::socket_relay::SocketRelayObserver;\n`,
    },
    ledger: [ledger("src/listener/supervisor.rs", "run", "tokio::spawn", "listener-supervisor", "owned().await", "src/listener/tests.rs", "cancellation_joins_children")],
  },
  {
    name: "unknown internal crate fails closed",
    expected: ["CRATE_UNKNOWN"],
    files: {
      "src-tauri/crates/new-layer/Cargo.toml": `[package]\nname = "intercept-proxy-new-layer"\nversion = "0.0.0"\n[dependencies]\n`,
    },
  },
  {
    name: "forbidden internal crate direction fails",
    expected: ["CRATE_DIRECTION"],
    files: {
      "src-tauri/crates/proxy/Cargo.toml": `[package]\nname = "intercept-proxy-runtime"\nversion = "0.0.0"\n[dependencies]\nintercept-proxy-domain = { path = "../domain" }\n`,
    },
  },
  {
    name: "neutral listener importing HTTP fails",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `use hyper::Request;\n` },
  },
  {
    name: "comment cannot forge cfg test masking",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `// #[cfg(test)]\nuse hyper::Request;\n` },
  },
  {
    name: "real cfg test HTTP import is ignored",
    expected: [],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)]\nmod tests { use hyper::Request; }\n` },
  },
  {
    name: "cfg test array return semicolon is ignored",
    expected: [],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] fn hidden() -> [u8; 1] { let _ = hyper::Request::new(()); [0] }\n` },
  },
  {
    name: "cfg test const array return is ignored",
    expected: [],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] fn hidden() -> [u8; { 1 }] { let _ = hyper::Request::new(()); [0] }\n` },
  },
  {
    name: "cfg test less-than const stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] const LESS: bool = 1 < 2;\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test less-than static stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] static LESS: bool = 1 < 2;\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test const-generic unit struct stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] struct Hidden<const B: bool = { 1 < 2 }>;\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test extern ABI fn stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] extern "C" fn hidden(_: crate::Arg) {}\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test unsafe extern block stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] unsafe extern "C" { fn hidden(_: crate::Arg); }\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test const unsafe fn stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] const unsafe fn hidden() {}\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test const extern ABI fn stops before production",
    expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] const extern "C" fn hidden() {}\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test macro_rules stops before production", expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] macro_rules! hidden { () => { hyper::Request::new(()) }; }\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test braced item macro stops before production", expected: ["NEUTRAL_HTTP"],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] hidden! { hyper::Request }\nuse hyper::Request;\n` },
  },
  {
    name: "cfg test return type macro is ignored", expected: [],
    files: { "src-tauri/crates/proxy/src/listener/handler.rs": `#[cfg(test)] fn hidden() -> ty!{} { let _ = hyper::Request::new(()); }\nuse tokio::net::TcpStream;\n` },
  },
  {
    name: "neutral transport importing Socket fails",
    expected: ["NEUTRAL_SOCKET"],
    files: { "src-tauri/crates/proxy/src/transport/relay.rs": `use crate::socket_relay::SocketRelayConfig;\n` },
  },
  {
    name: "HTTP importing Socket fails",
    expected: ["HTTP_SOCKET"],
    files: { "src-tauri/crates/proxy/src/http/parser.rs": `use crate::socket_relay::SocketRelayConfig;\n` },
  },
  ...socketForbidden.map(([code, pattern, label]) => ({
    name: `Socket importing ${label} fails`,
    expected: [code],
    files: { "src-tauri/crates/proxy/src/socket_relay/handler.rs": `${socketFixtureSource(pattern)}\n` },
  })),
  {
    name: "unregistered production spawn fails",
    expected: ["SPAWN_UNREGISTERED"],
    files: { "src-tauri/crates/proxy/src/other.rs": `pub async fn detach() { tokio::spawn(async {}); }\n` },
  },
  {
    name: "handler spawn cannot be legalized as a lifecycle facility",
    expected: ["SPAWN_OWNER"],
    files: {
      "src-tauri/crates/proxy/src/http/handler.rs": `pub async fn handle() { tokio::spawn(async { work().await }); }\n`,
      "src-tauri/crates/proxy/src/http/tests.rs": `fn cancellation_joins_children() {}\n`,
    },
    ledger: [ledger("src/http/handler.rs", "handle", "tokio::spawn", "lifecycle-facility", "work().await", "src/http/tests.rs", "cancellation_joins_children")],
  },
  {
    name: "registered debt fails in zero-debt mode",
    expected: ["SPAWN_DEBT"],
    zeroDebt: true,
    files: { "src-tauri/crates/proxy/src/forward/service/http.rs": `pub async fn forward_http() { tokio::spawn(async { legacy().await }); }\n` },
    debt: [debt("src/forward/service/http.rs", "forward_http", "legacy().await")],
  },
  {
    name: "registered debt passes during phased migration",
    expected: [],
    files: { "src-tauri/crates/proxy/src/forward/service/http.rs": `pub async fn forward_http() { tokio::spawn(async { legacy().await }); }\n` },
    debt: [debt("src/forward/service/http.rs", "forward_http", "legacy().await")],
  },
  {
    name: "stale debt row fails after migration",
    expected: ["SPAWN_DEBT_STALE"],
    files: { "src-tauri/crates/proxy/src/forward/service/http.rs": `pub async fn forward_http() { lexical().await; }\n` },
    debt: [debt("src/forward/service/http.rs", "forward_http", "legacy().await")],
  },
];

function socketFixtureSource(pattern) {
  const candidates = [
    "use hyper::Request;",
    "use crate::PipelinePorts;",
    "use crate::message::HttpMessage;",
    "use crate::Capture;",
    "use crate::BreakpointDecision;",
    "use crate::Mitm;",
    "use crate::BodyCodec;",
  ];
  return candidates.find((candidate) => pattern.test(candidate)) ?? "compile_error!(\"missing fixture\");";
}

async function runFixtureTests() {
  const temporaryRoot = await mkdtemp(path.join(os.tmpdir(), "runtime-architecture-"));
  try {
    for (const [index, fixture] of fixtureCases.entries()) {
      const fixtureRoot = path.join(temporaryRoot, String(index));
      await materializeFixture(fixtureRoot, fixture);
      const result = await scan(fixtureRoot, {
        ledgerEntries: fixture.ledger ?? [],
        debtEntries: fixture.debt ?? [],
        zeroDebt: fixture.zeroDebt ?? false,
      });
      const actual = result.violations.map(({ code }) => code).sort();
      const expected = [...fixture.expected].sort();
      if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        throw new Error(`fixture "${fixture.name}" expected [${expected.join(", ")}], got [${actual.join(", ")}]`);
      }
    }
  } finally {
    const temporaryStat = await stat(temporaryRoot);
    if (!temporaryStat.isDirectory() || !temporaryRoot.startsWith(os.tmpdir())) throw new Error("refusing to remove a non-temporary fixture path");
    await rm(temporaryRoot, { recursive: true });
  }
  console.log(`Runtime architecture fixtures passed (${fixtureCases.length} cases).`);
}

function printViolations(violations) {
  for (const violation of violations) {
    const location = `${violation.file}${violation.line ? `:${violation.line}` : ""}`;
    console.error(`- [${violation.code}] ${location}: ${violation.message}`);
  }
}

await runFixtureTests();
const result = await scan(repositoryRoot, {
  ledgerEntries: productionSpawnLedger,
  debtEntries: productionSpawnDebt,
  zeroDebt: requireZeroDebt,
});

if (result.violations.length > 0) {
  console.error("Runtime architecture gate failed:");
  printViolations(result.violations);
  process.exitCode = 1;
} else {
  console.log(`Runtime architecture gate passed (${result.sites.length} owned production task sites).`);
}

if (result.activeDebt.length > 0) {
  console.log(`Phase-1 spawn debt: ${result.activeDebt.length} site(s).`);
  for (const entry of result.activeDebt) console.log(`- ${entry.file} :: ${entry.symbol} :: ${entry.anchor}`);
  console.log("Clear each debt by routing it through ConnectionTaskScope::spawn_owned and deleting its productionSpawnDebt row.");
  console.log("Final gate: node scripts/check-runtime-architecture.mjs --require-zero-debt");
} else {
  console.log("Phase-1 spawn debt is zero.");
}

if (phase2LegacyHttpTransportDebt.size > 0) {
  console.log(`Phase-2 legacy HTTP transport debt: ${phase2LegacyHttpTransportDebt.size} file(s).`);
  for (const file of [...phase2LegacyHttpTransportDebt].sort()) console.log(`- ${file}`);
  console.log("Clear each transport debt by moving HTTP semantics under src/http, replacing it with neutral transport code, and deleting its phase2LegacyHttpTransportDebt row.");
}
