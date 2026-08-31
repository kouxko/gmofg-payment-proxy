import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase7-package-runtime.mjs");
const files = [
  "src-tauri/crates/infrastructure/src/package_transport.rs",
  "src-tauri/crates/infrastructure/src/package_transport/driver.rs",
  "src-tauri/crates/infrastructure/src/adapters/protocol_package_import.rs",
  "src-tauri/crates/infrastructure/Cargo.toml",
  "src-tauri/crates/infrastructure/src/lib.rs",
  "src-tauri/crates/package-runtime/src/lib.rs",
  "src-tauri/crates/infrastructure/src/adapters/external_package_server.rs",
  "src-tauri/crates/infrastructure/src/adapters/external_package_registry/mod.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/contract.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/capabilities.rs",
  "src-tauri/crates/proxy/src/socket_relay/protocol_exchange.rs",
  "src-tauri/crates/proxy/src/socket_relay/processing.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/socket_diagnostics.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/socket_diagnostics/mapping.rs",
  "test-support/fixtures/task-20260829-002/phase-7/package-runtime/inventory.json",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs",
];

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase7-package-runtime-"));
  for (const file of files) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  return target;
}

const discovery = JSON.stringify({
  archive: ["archive_entry_file_total_ratio_and_depth_limits_fail_closed", "declared_and_actual_entry_sizes_must_match", "directory_wrappers_typescript_and_non_js_payloads_are_rejected", "manifest_json_uses_the_shared_strict_contract", "missing_fixed_root_file_is_protocol_package_invalid", "root_manifest_protocol_display_and_relative_js_modules_are_accepted"],
  transport: ["cancelling_pre_registration_connect_drops_the_silent_peer", "fixed_decode_request_and_typed_result_use_shared_contract", "frame_result_is_validated_against_the_sent_buffer", "many_sequential_rpc_ids_do_not_accumulate_and_duplicate_reply_fails_closed", "null_document_is_a_present_success_result", "package_initiates_idless_registration_and_proxy_sends_no_reply", "raw_logical_frame_limit_is_independent_from_encoded_wire_budget"],
});

function run(cwd, discovered = discovery) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE7_CHECKER_TEST_MODE: "sandbox", PHASE7_DISCOVERY_JSON: discovered },
  });
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

function append(file, text) { return (target) => fs.appendFileSync(path.join(target, file), `\n${text}\n`); }
function replace(file, before, after) { return (target) => { const name = path.join(target, file); fs.writeFileSync(name, fs.readFileSync(name, "utf8").replace(before, after)); }; }

for (const [name, mutate, expected] of [
  ["proxy initiated registration", append(files[0], "fn bad(){ PackageRegisterNotification::new(todo!()); }"), /initiate or reply/i],
  ["registration reply", append(files[0], "type RegistrationReply = PackageRpcSuccess<PackageManifest>;"), /initiate or reply/i],
  ["dynamic public call", append(files[0], "impl PackageTransportClient { pub async fn call<T>(&self) {} }"), /dynamic public/i],
  ["dynamic method String", append(files[0], "struct Dynamic { method: String }"), /dynamic method String/i],
  ["second Manifest owner", append(files[0], "struct PackageManifest {}"), /second package contract owner/i],
  ["second typed DTO owner", append(files[0], "struct FrameParams {}"), /second package contract owner/i],
  ["hook timeout", append(files[0], "struct Limits { rpc_timeout: u64 }"), /timeout\/max-in-flight/i],
  ["hook max in flight", append(files[0], "struct Limits { max_in_flight: usize }"), /timeout\/max-in-flight/i],
  ["Busy policy", append(files[0], "enum Failure { Busy } impl Failure { fn x(&self){ match self { Self::Busy => {} } } }"), /Busy/i],
  ["retry policy", append(files[0], "fn retry_request() {}"), /retry/i],
  ["FrameResult buffer validation", replace(files[0], "validate_against_buffer_len", "unchecked_buffer_len"), /validated against/i],
  ["missing production import callsite", replace(files[2], /read_package_zip/g, "read_legacy_zip"), /active import/i],
  ["missing production dependency", replace(files[3], "intercept-proxy-package-runtime", "removed-package-runtime"), /must depend/i],
  ["canonical Base64 decode", replace(files[15], "CanonicalBase64::try_from", "CanonicalBase64::from_unchecked"), /canonical Base64/i],
  ["fixed ZIP root", replace(files[5], /display\.js/g, "view.js"), /display\.js/i],
  ["restored legacy allowlist", (target) => {
    const file = path.join(target, files[14]);
    const inventory = JSON.parse(fs.readFileSync(file, "utf8"));
    inventory.legacy_internal_allowlist = [{ file: "legacy.rs", symbol: "parse", owning_phase: "Phase13", reason: "must not return" }];
    fs.writeFileSync(file, `${JSON.stringify(inventory, null, 2)}\n`);
  }, /remain empty/i],
  ["unbounded ZIP entry read", replace(files[5], ".take(limits.max_file_bytes().saturating_add(1))", ".take(u64::MAX)"), /bounded actual-byte/i],
  ["dropped stable package code", replace(files[9], ".with_external_package_call", ".without_external_package_call"), /stable package error code/i],
  ["registration-only websocket ceiling", replace(files[6], "config.websocket_message_bytes()", "config.registration_websocket_message_bytes()"), /wire ceiling/i],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
  });
}

test("fails closed for Cargo zero-test discovery", () => {
  const target = sandbox();
  const result = run(target, JSON.stringify({ archive: [], transport: [] }));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Cargo discovered/i);
});

test("comments and strings do not create false legacy policy failures", () => {
  const target = sandbox();
  append(files[0], '// rpc_timeout max_in_flight Busy retry replay method: String\nconst HELP: &str = "PackageRegisterNotification::new";')(target);
  const result = run(target);
  assert.equal(result.status, 0, result.stderr);
});
