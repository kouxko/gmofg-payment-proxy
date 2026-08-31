import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";

const paths = {
  exchange: "src-tauri/crates/exchange/src/pipeline.rs",
  factory: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay.rs",
  capabilities: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/capabilities.rs",
  joint: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/joint_socket.rs",
  evaluation: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs",
  actor: "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  plan: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/plan/scripted.rs",
  tests: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/tests.rs",
  productionTest: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/tests/production_joint.rs",
  bindings: "src/generated/rust-types.ts",
  actorTest: "src-tauri/crates/infrastructure/src/adapters/pipeline/tests/socket_joint_transaction.rs",
  bundle: "src-tauri/crates/infrastructure/src/adapters/bundle.rs",
  runtimeSupport: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/external_package_runtime/support.rs",
  runtimeTest: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/external_package_runtime.rs",
  workspace: "src-tauri/crates/domain/src/workspace/runtime_projection.rs",
  repository: "src-tauri/crates/infrastructure/src/adapters/rules.rs",
  socketHandler: "src-tauri/crates/proxy/src/socket_relay/handler_support.rs",
  unifiedProjection: "src-tauri/crates/domain/src/workspace/unified_projection.rs",
};
const source = Object.fromEntries(await Promise.all(Object.entries(paths).map(async ([key, path]) => [key, await readFile(path, "utf8")])));
source.tests = `${source.tests}\n${source.productionTest}`;
const failures = [];
const requireText = (key, text, message) => { if (!source[key].includes(text)) failures.push(message); };
const forbid = (key, pattern, message) => { if (pattern.test(source[key])) failures.push(message); };

requireText("exchange", "consumed == 0 || consumed > buffer.len()", "Socket Frame consumedBytes must reject zero and oversized values");
requireText("exchange", 'failed_with_context::<Socket, D>("display"', "Socket Display failure must remain observable and fail-open");
for (const text of ["ExternalPackageRpc", "FrameParams", "DecodeParams", "DisplayParams"])
  requireText("capabilities", text, `Socket shared RPC capability missing ${text}`);
requireText("factory", "new_with_pipeline", "production Socket factory must receive the shared pipeline");
requireText("plan", "ExternalSocketCapabilityFactoryAdapter::new_with_pipeline", "production plan must inject shared pipeline ports");
for (const text of ["apply_socket_policy", "JointDocumentEvaluation::new_external_socket", "PreparedSocketEncode"])
  requireText("joint", text, `joint Socket transaction missing ${text}`);
for (const text of ["self.document == self.original_document", "CanonicalBase64::from_bytes(&original_input)", "SocketExternal"])
  requireText("evaluation", text, `Socket unchanged/changed Encode contract missing ${text}`);
for (const text of ["socket_joint", "joint.encode().await", "current.engine = checkpoint"])
  requireText("actor", text, `actor-owned Socket rollback contract missing ${text}`);
for (const text of ["socket_encode_failure_rolls_back_lifecycle_before_successful_commit", "commit_attempts", "hit_count, 0", "hit_count, 1"])
  requireText("actorTest", text, `Socket actor lifecycle test missing ${text}`);
for (const text of ["ListenerRuntimePipelineAssembly", "configure_listener_runtime_pipeline"])
  requireText("bundle", text, `single production listener pipeline assembly missing ${text}`);
for (const text of ["ListenerRuntimePipelineAssembly", "RuleRepositoryAdapter::new", "configure_listener_runtime_pipeline"])
  requireText("runtimeSupport", text, `production runtime fixture must use real assembly dependency ${text}`);
for (const text of [
  "runtime_rule_execution_order",
  "pub fn runtime_rules(&self)",
  "actor_owned_socket_conditions(&content.condition)?",
  "replace_runtime_rule_lifecycle",
  "reset_runtime_rule_hit_metadata",
]) requireText("workspace", text, `Workspace unified HTTP + Socket runtime projection missing ${text}`);
for (const text of ["actor_owned_socket_conditions", "Condition::NthHit { count", "Condition::Document { .. }"])
  requireText("unifiedProjection", text, `Socket actor-owned NthHit projection missing ${text}`);
for (const text of [
  "workspace.runtime_rules()?",
  "workspace.runtime_rule_execution_order()",
  "replace_runtime_rule_lifecycle",
  "if workspace.reset_runtime_rule_hit_metadata()?",
]) requireText("repository", text, `RuleRepository unified runtime persistence missing ${text}`);
requireText("socketHandler", "runtime_epoch: run.workspace_runtime_epoch", "Socket actor identity must use Workspace runtime epoch");
forbid("socketHandler", /runtime_epoch:\s*run\.listener_run_epoch/u, "Socket actor identity must not use listener supervisor epoch");
for (const text of [
  "production_socket_pipeline_rolls_back_failure_and_commits_each_write_stage_once",
  'assert_eq!(harness.peer().encode_methods(), ["hooks.upstream.encode"])',
  "Condition::NthHit { count: 2 }",
  "initial_revision + 1",
  "initial_revision + 3",
  "after_upstream_commit.revision.get(), initial_revision + 2",
  '"hooks.downstream.encode"',
  "hit_count == 0",
  "hit_count == 1",
]) requireText("runtimeTest", text, `real production Socket/SQLite transaction test missing ${text}`);
const stageFixture = source.runtimeTest.match(/fn two_stage_one_shot_rules[\s\S]*?\.collect\(\)\n\}/u)?.[0] ?? "";
const stageSequence = [...stageFixture.matchAll(/ProtocolRuleStage::(\w+)/gu)].map((match) => match[1]);
if (stageSequence.join(",") !== "ProxyToUpstream,ProxyToApp") {
  failures.push("production Relay fixture must use the two authoritative distinct write stages in order");
}
forbid("joint", /retry|replay|queue_capacity|rpc_timeout|max_in_flight/u, "Socket transaction must not add timeout, queue, retry, or replay");
for (const text of [
  "production_joint_pipeline_preserves_unchanged_bytes_without_encode_rpc",
  "production_joint_pipeline_changes_document_before_encode_rpc",
  "production_joint_pipeline_encode_failure_preserves_typed_identity",
]) requireText("tests", text, `Cargo Phase11 test missing ${text}`);
const socketRule = source.bindings.match(/export type SocketRuleContent = \{[\s\S]*?\n\};/u)?.[0] ?? "";
if (!socketRule || /\b(?:headers|method|status|url)\b/u.test(socketRule)) {
  failures.push("generated Socket rule adapter must not expose HTTP-only fields");
}

if (process.env.PHASE11_CHECKER_TEST_MODE !== "sandbox") {
  const result = spawnSync("cargo", [
    "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure",
    "production_joint::production_joint_pipeline", "--", "--list", "--format", "terse",
  ], { encoding: "utf8" });
  const count = result.status === 0
    ? result.stdout.split("\n").filter((line) => line.includes("production_joint_pipeline") && line.endsWith(": test")).length
    : 0;
  if (count !== 3) failures.push(`Cargo discovery expected 3 Phase11 production tests, found ${count}`);
}

if (failures.length) {
  for (const failure of failures) console.error(`FAIL: ${failure}`);
  process.exit(1);
}
console.log("PASS: Phase 11 Socket pipeline contract");
