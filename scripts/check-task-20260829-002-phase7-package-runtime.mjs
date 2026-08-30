import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import process from "node:process";

const read = (file) => readFile(file, "utf8");
const strip = (value) => value.replace(/\/\*[\s\S]*?\*\//gu, " ").replace(/\/\/[^\n]*/gu, " ").replace(/r#*"[\s\S]*?"#*/gu, "\"\"").replace(/"(?:\\.|[^"\\])*"/gu, "\"\"");
const paths = {
  transport: "src-tauri/crates/infrastructure/src/package_transport.rs",
  transportDriver: "src-tauri/crates/infrastructure/src/package_transport/driver.rs",
  importer: "src-tauri/crates/infrastructure/src/adapters/protocol_package_import.rs",
  infrastructureCargo: "src-tauri/crates/infrastructure/Cargo.toml",
  infrastructureRoot: "src-tauri/crates/infrastructure/src/lib.rs",
  packageRuntime: "src-tauri/crates/package-runtime/src/lib.rs",
  server: "src-tauri/crates/infrastructure/src/adapters/external_package_server.rs",
  registry: "src-tauri/crates/infrastructure/src/adapters/external_package_registry/mod.rs",
  relay: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/contract.rs",
  capabilities: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/capabilities.rs",
  protocolExchange: "src-tauri/crates/proxy/src/socket_relay/protocol_exchange.rs",
  processing: "src-tauri/crates/proxy/src/socket_relay/processing.rs",
  socketDiagnostics: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/socket_diagnostics.rs",
  socketDiagnosticMapping: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/socket_diagnostics/mapping.rs",
  transportConfig: "src-tauri/crates/infrastructure/src/package_transport.rs",
};
const source = Object.fromEntries(await Promise.all(Object.entries(paths).map(async ([key, file]) => [key, await read(file)])));
const code = Object.fromEntries(Object.entries(source).map(([key, value]) => [key, strip(value)]));
const transport = `${code.transport}\n${code.transportDriver}`;
const failures = [];
const failIf = (condition, message) => { if (condition) failures.push(message); };

failIf(/PackageRegisterNotification::new|PackageRpcRequest::PackageRegister|package\.register[^\n]*(?:send|Message::Text)|PackageRpcSuccess\s*<\s*PackageManifest/.test(transport), "proxy must not initiate or reply to package.register");
failIf(!/from_str\s*::\s*<\s*PackageRegisterNotification\s*>/.test(transport), "registration must deserialize the shared idless notification");
failIf(/pub\s+async\s+fn\s+call(?:_display)?\s*</.test(transport), "dynamic public hook calls are forbidden");
const active = [transport, code.server, code.registry, code.relay, code.capabilities].join("\n");
failIf(/method\s*:\s*String/.test(active), "active package transport must not own a dynamic method String");
failIf(/rpc_timeout|max_in_flight|Semaphore|Self::Busy|Busy\s*=>|Self::Retry|Retry\s*=>|Self::Replay|Replay\s*=>|retry_|replay_/.test(transport), "new hook transport must not expose timeout/max-in-flight/Busy/retry/replay policy");
failIf(!code.transport.includes("validate_against_buffer_len"), "frame response must be validated against the sent buffer");
failIf(!code.capabilities.includes("CanonicalBase64::from_bytes") || !code.capabilities.includes("try_into()"), "active Socket adapter must enforce canonical Base64 in both directions");
for (const method of ["upstream_frame", "downstream_frame", "upstream_decode", "downstream_decode", "upstream_encode", "downstream_encode", "upstream_display", "downstream_display"]) failIf(!code.transport.includes(`fn ${method}`), `missing fixed typed client method ${method}`);
failIf(/ExternalPackageClient|ExternalPackageConnectionConfig|ExternalPackageConnectionError/.test(code.infrastructureRoot), "legacy dynamic transport must not be re-exported");
failIf(/\bExternalPackageRegistration\b|\bExternalFrameRequest\b|\bExternalDecodeRequest\b|\bExternalEncodeRequest\b|\bExternalDisplayRequest\b|\.call\s*\(/.test(active), "active runtime still consumes a legacy dynamic DTO or call path");
for (const owner of ["PackageManifest", "FrameParams", "DecodeParams", "EncodeParams", "DisplayParams", "FrameResult", "PackageRpcRequest"]) {
  failIf(new RegExp(`(?:struct|enum)\\s+${owner}\\b`).test(active), `second package contract owner detected for ${owner}`);
  failIf(new RegExp(`type\\s+\\w+\\s*=\\s*${owner}\\b`).test(active), `compatibility alias detected for ${owner}`);
}
failIf(!code.packageRuntime.includes("PackageManifest"), "ZIP parser must consume the shared PackageManifest owner");
failIf(!code.importer.includes("read_package_zip") || code.importer.includes("repository.prepare_zip"), "active import must call the strict package runtime and must not fall back to legacy ZIP preparation");
failIf(!source.infrastructureCargo.includes("intercept-proxy-package-runtime"), "infrastructure must depend on the strict package runtime");
for (const required of ["manifest.json", "protocol.js", "display.js"]) failIf(!source.packageRuntime.includes(required), `ZIP parser must require ${required}`);
failIf(!source.packageRuntime.includes(".take(limits.max_file_bytes().saturating_add(1))") || !source.packageRuntime.includes("actual_size != declared_size") || !source.packageRuntime.includes("checked_add(actual_size)"), "ZIP entries must use bounded actual-byte accounting and reject declared-size mismatch");
failIf(!code.capabilities.includes("with_external_package_call") || !code.protocolExchange.includes("clone_from(&error.external_package_call)") || !code.processing.includes("external_package_call: Option<Box<ExternalPackageCallFailure>>") || !code.socketDiagnostics.includes("map(external_package_call_view)") || !code.socketDiagnosticMapping.includes("stable_code: call.stable_code.clone()"), "stable package error code must traverse the active Socket failure and diagnostic chain");
failIf(!source.server.includes("config.websocket_message_bytes()") || !source.transportConfig.includes("max_registration_message_bytes > self.max_rpc_message_bytes") || !source.transportConfig.includes("max_display_message_bytes"), "production WebSocket must use the registration/RPC/display wire ceiling");

const inventory = JSON.parse(await read("test-support/fixtures/task-20260829-002/phase-7/package-runtime/inventory.json"));
const allowlist = inventory.legacy_internal_allowlist ?? [];
failIf(allowlist.length !== 1, "legacy allowlist must be narrow and exact");
const entry = allowlist[0];
if (entry) {
  failIf(entry.file !== "src-tauri/crates/protocol-scripting/src/lib.rs" || entry.symbol !== "parse_protocol_manifest" || entry.owning_phase !== "Phase13" || typeof entry.reason !== "string" || entry.reason.length < 24, "legacy allowlist must use exact file, symbol, reason and owning phase");
  if (entry.file === "src-tauri/crates/protocol-scripting/src/lib.rs") failIf(!(await read(entry.file)).includes(entry.symbol), `${entry.file}#${entry.symbol}: stale legacy allowlist entry`);
}

function discover(args) {
  const result = spawnSync("cargo", args, { encoding: "utf8" });
  if (result.status !== 0) return [];
  return result.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6)).sort();
}
const discovered = process.env.PHASE7_CHECKER_TEST_MODE === "sandbox" ? JSON.parse(process.env.PHASE7_DISCOVERY_JSON ?? "{}") : {
  archive: discover(["test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-package-runtime", "--test", "phase7_archive_contract", "--", "--list", "--format", "terse"]),
  transport: discover(["test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure", "--test", "phase7_transport_contract", "--", "--list", "--format", "terse"]),
};
for (const kind of ["archive", "transport"]) {
  const expected = [...(inventory.discoverable_tests?.[kind] ?? [])].sort();
  failIf(expected.length === 0 || JSON.stringify(discovered[kind] ?? []) !== JSON.stringify(expected), `Cargo discovered ${kind} tests drift or zero-test target`);
}

if (failures.length) { for (const failure of failures) console.error(`FAIL: ${failure}`); process.exit(1); }
console.log("PASS: Phase 7 package runtime contract");
