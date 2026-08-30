import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase10-http-pipeline.mjs");
const files = [
  "src-tauri/crates/exchange/src/protocol.rs",
  "src-tauri/crates/proxy/src/http/exchange_runtime/endpoints.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/external_http.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/joint_rules.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  "src-tauri/crates/proxy/src/lib.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/legacy_http.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline/production_shape.rs",
  "src-tauri/crates/exchange/src/pipeline.rs",
];

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase10-http-"));
  for (const file of files) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  return target;
}

function run(cwd) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE10_CHECKER_TEST_MODE: "sandbox" },
  });
}

function replace(file, before, after) {
  return (target) => {
    const name = path.join(target, file);
    fs.writeFileSync(name, fs.readFileSync(name, "utf8").split(before).join(after));
  };
}

function append(file, text) {
  return (target) => fs.appendFileSync(path.join(target, file), `\n${text}\n`);
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, mutate, expected] of [
  ["wire body removed", replace(files[0], "wire_body", "projected_body"), /original wire/u],
  ["endpoint loses wire bytes", replace(files[1], "wire_body: message.body.to_vec()", "wire_body: Vec::new()"), /authoritative body/u],
  ["shared provider removed", replace(files[2], "external_package_provider", "legacy_package_provider"), /shared online/u],
  ["HTTP kind gate removed", replace(files[2], "PackageKind::Http", "PackageKind::Socket"), /non-HTTP/u],
  ["charset gate removed", replace(files[3], "HTTP_BODY_CHARSET_UNSUPPORTED", "BODY_MAY_BE_LOSSY"), /charset/u],
  ["content encoding gate removed", replace(files[3], "HTTP_CONTENT_ENCODING_UNSUPPORTED", "CONTENT_ENCODING_ACCEPTED"), /Content-Encoding/u],
  ["legacy runtime added", append(files[3], "fn wrong() { DirectionExecutionPlan::default(); }"), /shared package RPC/u],
  ["retry policy added", append(files[3], "fn retry_with_queue_capacity() {}"), /must not add timeout/u],
  ["unchanged gate removed", replace(files[4], "self.document == self.original_document", "false"), /wire contract/u],
  ["encode RPC removed", replace(files[4], ".encode(", ".display("), /wire contract/u],
  ["joint actor typed failure removed", replace(files[5], ".with_external_package_call(error.external_package_call)", ".with_external_package_call(None)"), /typed package failure/u],
  ["legacy cfg(test) containment removed", replace(files[2], "#[cfg(test)]\nmod legacy_http;", "mod legacy_http;"), /cfg\(test\)-contained/u],
  ["actor external code folded to internal", replace(files[5], "ErrorCode::ExternalPackageCallFailed", "ErrorCode::Internal"), /top-level external package error code/u],
  ["proxy external code removed", replace(files[6], "ExternalPackageCallFailed => \"EXTERNAL_PACKAGE_CALL_FAILED\"", "ExternalPackageCallFailed => \"INTERNAL_ERROR\""), /external package failure classification/u],
  ["display fail-open observation removed", replace(files[10], "failed_with_context::<Http, D>(\"display\"", "failed_with_context::<Http, D>(\"ignored\""), /Display fail-open/u],
  ["endpoint typed error removed", replace(files[1], ".with_external_package_call(*failure)", ""), /preserve typed Proxy/u],
  ["unchanged behavior test removed", replace(files[8], "unchanged_external_document_forwards_original_wire_bytes_without_encode_rpc", "missing_unchanged_case"), /Cargo Phase10 test/u],
  ["changed behavior test removed", replace(files[8], "changed_external_document_uses_encode_rpc_and_encode_failure_fails_closed", "missing_changed_case"), /Cargo Phase10 test/u],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
  });
}
