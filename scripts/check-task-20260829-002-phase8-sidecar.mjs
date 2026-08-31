import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import process from "node:process";

const read = (file) => readFile(file, "utf8");
const strip = (value) =>
  value
    .replace(/\/\*[\s\S]*?\*\//gu, " ")
    .replace(/\/\/[^\n]*/gu, " ")
    .replace(/r#*"[\s\S]*?"#*/gu, "\"\"")
    .replace(/"(?:\\.|[^"\\])*"/gu, "\"\"");

const paths = {
  workspaceCargo: "src-tauri/Cargo.toml",
  runtimeCargo: "src-tauri/crates/package-runtime/Cargo.toml",
  runtimeRoot: "src-tauri/crates/package-runtime/src/lib.rs",
  sidecar: "src-tauri/crates/package-runtime/src/sidecar.rs",
  sidecarBin: "src-tauri/crates/package-runtime/src/bin/intercept-proxy-package-sidecar.rs",
  phase8Test: "src-tauri/crates/package-runtime/tests/phase8_sidecar_runtime.rs",
  phase8ReviewTest: "src-tauri/crates/package-runtime/tests/phase8_sidecar_review.rs",
  phase7Checker: "scripts/check-task-20260829-002-phase7-package-runtime.mjs",
  tauriConfig: "src-tauri/tauri.conf.json",
};

const source = Object.fromEntries(
  await Promise.all(Object.entries(paths).map(async ([key, file]) => [key, await read(file)])),
);
const code = Object.fromEntries(Object.entries(source).map(([key, value]) => [key, strip(value)]));
const failures = [];
const failIf = (condition, message) => {
  if (condition) failures.push(message);
};

failIf(!source.workspaceCargo.includes('boa_engine = "=0.22.0"'), "Boa dependency must be pinned without disabling native default features");
failIf(/boa_engine\s*=\s*\{[^}]*default-features\s*=\s*false/u.test(source.workspaceCargo), "Boa native default features must not be disabled");
failIf(!source.runtimeCargo.includes("boa_engine.workspace = true"), "package-runtime must consume workspace Boa");
failIf(!source.runtimeCargo.includes("[[bin]]") || !source.runtimeCargo.includes("intercept-proxy-package-sidecar"), "generic sidecar binary target is missing");
const phase9Active = existsSync("scripts/check-task-20260829-002-phase9-lifecycle.mjs");
failIf(!source.sidecarBin.includes("sidecar_executable_marker"), "sidecar executable must compile through package-runtime without inventing Phase9 launch policy");
failIf(!source.runtimeRoot.includes("mod sidecar") || !source.runtimeRoot.includes("pub use sidecar::*"), "package-runtime must expose the sidecar runtime from one owner module");

for (const fragment of ["PackageModuleLoader", "resolve_module_specifier", "Module::parse", "load_link_evaluate", "run_jobs", "get_value", "is_callable"]) {
  failIf(!source.sidecar.includes(fragment), `Boa module lifecycle/export validation missing ${fragment}`);
}
failIf(!source.sidecar.includes('specifier.starts_with("./")') || !source.sidecar.includes('specifier.starts_with("../")'), "package imports must be restricted to relative ESM specifiers");
failIf(!source.sidecar.includes("HTTP_PROTOCOL_EXPORTS") || !source.sidecar.includes("SOCKET_PROTOCOL_EXPORTS"), "HTTP and Socket required exports must follow their distinct fixed contracts");
failIf(!source.sidecar.includes("exports: BTreeMap") || !source.sidecar.includes("cache_exports"), "fixed Sidecar exports must be cached after registration precheck");
failIf(!source.sidecar.includes("value.as_promise()") || !source.sidecar.includes("PromiseState::Pending") || !source.sidecar.includes("loop {"), "Boa Promise hook results, including dynamic import, must be driven until settled without timeout or Busy policy");
for (const exportName of ["upstreamFrame", "downstreamFrame", "upstreamDecode", "downstreamDecode", "upstreamEncode", "downstreamEncode", "upstreamDisplay", "downstreamDisplay"]) {
  failIf(!source.sidecar.includes(exportName), `missing fixed Sidecar export ${exportName}`);
}
for (const publicMethod of ["upstream_frame", "downstream_frame", "upstream_decode", "downstream_decode", "upstream_encode", "downstream_encode", "upstream_display", "downstream_display"]) {
  failIf(!source.sidecar.includes(`pub fn ${publicMethod}`), `missing fixed runtime method ${publicMethod}`);
}
failIf(!source.sidecar.includes("JsUint8Array::from_iter") || !source.sidecar.includes("JsUint8Array::from_object") || !source.sidecar.includes("CanonicalBase64::try_from") || !source.sidecar.includes("CanonicalBase64::from_bytes"), "Socket Base64 must be adapted through exact Uint8Array values internally");
failIf(
  /\bregister_global_[A-Za-z0-9_]*\s*\(|\bNativeFunction\b|\bHostHooks\b|\.host_hooks\s*\(/u.test(code.sidecar),
  "Proxy must not inject non-Boa Host binding injection through register_global_*, NativeFunction or custom HostHooks",
);
failIf(/rpc_timeout|max_in_flight|Semaphore|retry_|replay_|Busy/.test(code.sidecar), "Sidecar runtime must not add hook timeout, queue, Busy, retry or replay policy");
failIf(/rhai|manifest\.toml|protocol\.rhai|display\.rhai/.test(code.sidecar), "Phase8 Sidecar runtime must not reuse Rhai/TOML");
failIf(/pub\s+fn\s+(?:test_export_json|call_export|invoke_export)/u.test(code.sidecar), "Sidecar runtime must not expose an arbitrary export invocation API");
failIf(!phase9Active && /std::env|args\(|listen|connect|spawn|Command|WebSocket|registration_deadline|heartbeat/u.test(code.sidecarBin), "generic Phase8 executable must not invent Phase9 process or transport lifecycle");
const tauri = JSON.parse(source.tauriConfig);
failIf(
  JSON.stringify(tauri.bundle?.externalBin) !== JSON.stringify(["binaries/intercept-proxy-package-sidecar"]),
  "Tauri externalBin must bundle the generic Sidecar through the current packaging lifecycle",
);

failIf(!source.phase8Test.includes("required_exports_are_prechecked_without_calling_package_code"), "Phase8 tests must prove export precheck does not trial-call hooks");
failIf(!source.phase8Test.includes("relative_esm_modules_are_evaluated_once_and_exports_are_cached"), "Phase8 tests must prove relative ESM one-time evaluation and cached exports");
failIf(!source.phase8Test.includes("fixed_exports_are_cached_after_registration_precheck"), "Phase8 tests must prove fixed export objects are cached after precheck");
failIf(!source.phase8Test.includes("only_package_relative_esm_specifiers_are_accepted"), "Phase8 tests must reject non-relative ESM specifiers");
failIf(!source.phase8Test.includes("http_hooks_receive_and_return_unicode_strings_without_socket_frame_exports"), "Phase8 tests must prove the HTTP string and kind-specific export contract");
failIf(!source.phase8Test.includes("socket_base64_is_presented_to_javascript_as_uint8array_and_returns_canonical_base64"), "Phase8 tests must prove Socket Uint8Array/Base64 boundary");
failIf(!source.phase8Test.includes("socket_encode_rejects_non_uint8array_results"), "Phase8 tests must reject non-Uint8Array Socket encode results");
failIf(!source.phase8Test.includes("all_eight_fixed_exports_map_to_the_matching_direction"), "Phase8 tests must prove all eight fixed direction mappings");
for (const reviewTest of ["dynamic_import_uses_boa_loader_and_evaluates_the_lazy_module_once", "nested_parent_imports_and_static_cycles_evaluate_each_module_once", "relative_imports_cannot_escape_the_package_root"]) {
  failIf(!source.phase8ReviewTest.includes(reviewTest), `Phase8 review tests missing ${reviewTest}`);
}
failIf(source.phase7Checker.includes("boa_engine") || source.phase7Checker.includes("intercept-proxy-package-sidecar"), "Phase8 ownership must not be folded into the Phase7 checker");

function discover(target) {
  const result = spawnSync("cargo", [
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "-p",
    "intercept-proxy-package-runtime",
    "--test",
    target,
    "--",
    "--list",
    "--format",
    "terse",
  ], { encoding: "utf8" });
  if (result.status !== 0) return [];
  return result.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6)).sort();
}

const discovered = process.env.PHASE8_CHECKER_TEST_MODE === "sandbox"
  ? JSON.parse(process.env.PHASE8_DISCOVERY_JSON ?? "{}")
  : {
      runtime: discover("phase8_sidecar_runtime"),
      review: discover("phase8_sidecar_review"),
    };
const expected = {
  runtime: [
  "all_eight_fixed_exports_map_to_the_matching_direction",
  "fixed_exports_are_cached_after_registration_precheck",
  "http_hooks_receive_and_return_unicode_strings_without_socket_frame_exports",
  "missing_or_non_callable_fixed_exports_fail_before_registration",
  "only_package_relative_esm_specifiers_are_accepted",
  "relative_esm_modules_are_evaluated_once_and_exports_are_cached",
  "required_exports_are_prechecked_without_calling_package_code",
  "socket_base64_is_presented_to_javascript_as_uint8array_and_returns_canonical_base64",
  "socket_encode_rejects_non_uint8array_results",
  ].sort(),
  review: [
    "dynamic_import_uses_boa_loader_and_evaluates_the_lazy_module_once",
    "nested_parent_imports_and_static_cycles_evaluate_each_module_once",
    "relative_imports_cannot_escape_the_package_root",
  ].sort(),
};
failIf(JSON.stringify(discovered) !== JSON.stringify(expected), "Cargo discovered Phase8 sidecar tests drift or zero-test target");

if (failures.length) {
  for (const failure of failures) console.error(`FAIL: ${failure}`);
  process.exit(1);
}
console.log("PASS: Phase 8 Boa Sidecar runtime contract");
