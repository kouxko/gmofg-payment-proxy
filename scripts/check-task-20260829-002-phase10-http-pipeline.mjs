import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";

const paths = {
  context: "src-tauri/crates/exchange/src/protocol.rs",
  endpoints: "src-tauri/crates/proxy/src/http/exchange_runtime/endpoints.rs",
  pipeline: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline.rs",
  external: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/external_http.rs",
  joint: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs",
  actor: "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  proxy: "src-tauri/crates/proxy/src/lib.rs",
  test: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline.rs",
  productionTest: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline/production_shape.rs",
  requestMetadataTest: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/phase10_http_pipeline/request_metadata.rs",
  exchangePipeline: "src-tauri/crates/exchange/src/pipeline.rs",
};
const source = Object.fromEntries(await Promise.all(Object.entries(paths).map(async ([key, file]) => [key, await readFile(file, "utf8")])));
source.test = `${source.test}\n${source.productionTest}\n${source.requestMetadataTest}`;
const failures = [];
const requireText = (key, text, message) => { if (!source[key].includes(text)) failures.push(message); };
const forbid = (key, pattern, message) => { if (pattern.test(source[key])) failures.push(message); };

requireText("context", "wire_body", "HTTP Context must retain original wire body bytes");
requireText("endpoints", "wire_body: message.body.to_vec()", "HTTP endpoint must preserve authoritative body bytes");
requireText("pipeline", "external_package_provider", "HTTP pipeline must resolve the shared online package provider");
requireText("pipeline", "PackageKind::Http", "HTTP pipeline must reject a non-HTTP package registration");
if (source.pipeline.includes("legacy_http") || existsSync("src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline/legacy_http.rs")) failures.push("legacy HTTP runtime must remain removed after Phase 13");
requireText("actor", ".with_external_package_call(error.external_package_call)", "joint actor must preserve typed package failure");
requireText("actor", "ErrorCode::ExternalPackageCallFailed", "joint actor must preserve the top-level external package error code");
requireText("proxy", "ExternalPackageCallFailed => \"EXTERNAL_PACKAGE_CALL_FAILED\"", "proxy error code must expose the external package failure classification");
requireText("exchangePipeline", "failed_with_context::<Http, D>(\"display\"", "HTTP Display fail-open must emit typed observation evidence");
requireText("exchangePipeline", "failed_with_context::<Socket, D>(\"display\"", "Socket Display fail-open must emit typed observation evidence");
requireText("endpoints", "with_external_package_call(*failure)", "HTTP endpoint must preserve typed Proxy error details into Exchange");
for (const text of ["ExternalPackageRpc", "DecodeParams", "DisplayParams", "decode_http_body_for_package"])
  requireText("external", text, `HTTP shared RPC pipeline missing ${text}`);
requireText("external", "HTTP_CONTENT_ENCODING_UNSUPPORTED", "non-identity Content-Encoding gate missing");
requireText("external", "HTTP_BODY_CHARSET_UNSUPPORTED", "strict HTTP charset gate missing");
for (const text of ["original_document", "self.document == self.original_document", ".encode(", "EncodeParams"])
  requireText("joint", text, `unchanged/changed Document wire contract missing ${text}`);
forbid("external", /ProtocolDirectionExecutor|DirectionExecutionPlan|LocalSidecarRuntime/u, "HTTP production pipeline must use only the shared package RPC");
forbid("external", /retry|replay|rpc_timeout|max_in_flight|queue_capacity/u, "HTTP package path must not add timeout, queue, retry or replay policy");
for (const text of [
  "strict_http_package_codec_reads_original_utf8_and_shift_jis_wire_bytes",
  "http_package_codec_rejects_unknown_charset_and_non_identity_content_encoding",
  "unchanged_external_document_forwards_original_wire_bytes_without_encode_rpc",
  "changed_external_document_uses_encode_rpc_and_encode_failure_fails_closed",
  "production_snapshot_uses_shared_provider_for_both_directions_and_joint_encode",
  "remote_decode_and_display_failures_keep_typed_json_rpc_identity",
  ".apply_request_policy(",
  "commit_attempts.load(Ordering::SeqCst), 0",
  "intercept_proxy_runtime::ErrorCode::Internal.as_str()",
  "assert_production_changed_commit",
  "assert_production_encode_failure_rolls_back",
  "production_response_rule_matches_recursive_tree_against_associated_request_metadata",
])
  requireText("test", text, `Cargo Phase10 test missing ${text}`);

const requiredCargoTests = [
  "changed_external_document_uses_encode_rpc_and_encode_failure_fails_closed",
  "http_package_codec_rejects_unknown_charset_and_non_identity_content_encoding",
  "production_http_joint_leaves_ordinary_false_rule_to_actor_matching",
  "production_http_actor_owns_unified_nth_attempt_and_one_shot_commit",
  "production_snapshot_compiles_recursive_or_with_insert_and_append",
  "production_snapshot_uses_shared_provider_for_both_directions_and_joint_encode",
  "remote_decode_and_display_failures_keep_typed_json_rpc_identity",
  "production_response_rule_matches_recursive_tree_against_associated_request_metadata",
  "strict_http_package_codec_reads_original_utf8_and_shift_jis_wire_bytes",
  "unchanged_external_document_forwards_original_wire_bytes_without_encode_rpc",
];
if (process.env.PHASE10_CHECKER_TEST_MODE !== "sandbox" || process.env.PHASE10_DISCOVERY_NAMES) {
  const injected = process.env.PHASE10_DISCOVERY_NAMES?.split(",").filter(Boolean);
  const result = spawnSync("cargo", [
    "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure",
    "phase10_http_pipeline_tests", "--", "--list", "--format", "terse",
  ], { encoding: "utf8" });
  const discovered = injected
    ? injected
    : result.status === 0
    ? result.stdout.split("\n")
      .filter((line) => line.includes("phase10_http_pipeline_tests") && line.endsWith(": test"))
      .map((line) => line.slice(0, -": test".length).split("::").at(-1))
    : [];
  for (const required of requiredCargoTests) {
    if (!discovered.includes(required)) failures.push(`Cargo discovery missing required Phase10 test ${required}`);
  }
}

if (failures.length) {
  for (const failure of failures) console.error(`FAIL: ${failure}`);
  process.exit(1);
}
console.log("PASS: Phase 10 HTTP pipeline contract");
