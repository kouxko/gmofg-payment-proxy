import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase11-socket-pipeline.mjs");
const files = [
  "src-tauri/crates/exchange/src/pipeline.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/capabilities.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/joint_socket.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/joint_document.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/plan/scripted.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/tests.rs",
  "src/generated/rust-types.ts",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/tests/socket_joint_transaction.rs",
  "src-tauri/crates/infrastructure/src/adapters/bundle.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/external_package_runtime/support.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/external_package_runtime.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs",
  "src-tauri/crates/proxy/src/socket_relay/handler_support.rs",
];
function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase11-socket-"));
  for (const file of files) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  return target;
}
function run(cwd) {
  return spawnSync(process.execPath, ["run", "-A", checker], {
    cwd, encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE11_CHECKER_TEST_MODE: "sandbox" },
  });
}
function replace(file, before, after) {
  return (target) => {
    const name = path.join(target, file);
    fs.writeFileSync(name, fs.readFileSync(name, "utf8").split(before).join(after));
  };
}
test("canonical repository passes", () => assert.equal(run(root).status, 0));
for (const [name, mutate] of [
  ["zero consumed accepted", replace(files[0], "consumed == 0 || consumed > buffer.len()", "consumed > buffer.len()")],
  ["display observation removed", replace(files[0], 'failed_with_context::<Socket, D>("display"', 'failed_with_context::<Socket, D>("ignored"')],
  ["pipeline injection removed", replace(files[6], "ExternalSocketCapabilityFactoryAdapter::new_with_pipeline", "ExternalSocketCapabilityFactoryAdapter::new")],
  ["joint policy removed", replace(files[3], "apply_socket_policy", "bypass_socket_policy")],
  ["unchanged gate removed", replace(files[4], "self.document == self.original_document", "false")],
  ["canonical base64 removed", replace(files[4], "CanonicalBase64::from_bytes(&original_input)", "CanonicalBase64::from_bytes(&[])")],
  ["actor encode removed", replace(files[5], "joint.encode().await", "unreachable!()")],
  ["retry added", replace(files[3], "//! Socket Document", "fn retry_with_queue_capacity() {}\n//! Socket Document")],
  ["unchanged production assertion removed", replace(files[12], "assert!(harness.peer().encode_methods().is_empty())", "assert!(false)")],
  ["changed production assertion removed", replace(files[12], 'assert_eq!(harness.peer().encode_methods(), ["hooks.upstream.encode"])', "assert!(false)")],
  ["typed Encode failure trigger removed", replace(files[12], "harness.peer().fail_encode_once()", "missing_fail_encode_once()")],
  ["HTTP field added to Socket adapter", replace(files[8], "\tactions: UnifiedAction[]", "\theaders: string[]\n\tactions: UnifiedAction[]")],
  ["actor rollback test removed", replace(files[9], "socket_encode_failure_rolls_back_lifecycle_before_successful_commit", "missing_actor_rollback")],
  ["production assembly removed", replace(files[10], "ListenerRuntimePipelineAssembly", "MissingPipelineAssembly")],
  ["runtime fixture bypasses production assembly", replace(files[11], "configure_listener_runtime_pipeline", "configure_test_pipeline")],
  ["real SQLite transaction test removed", replace(files[12], "production_socket_pipeline_rolls_back_failure_and_commits_each_write_stage_once", "missing_real_transaction")],
  ["actor hot replace removed", replace(files[5], "current.snapshot = snapshot", "drop(snapshot)")],
  ["actor lifecycle checkpoint removed", replace(files[5], "let checkpoint = current.clone()", "let checkpoint = RuleRuntime::default()")],
  ["RuleDefinition actor owner removed", replace(files[13], "rules: &[RuleDefinition]", "rules: &[LegacyRule]")],
  ["Socket actor uses listener epoch", replace(files[14], "runtime_epoch: run.workspace_runtime_epoch", "runtime_epoch: run.listener_run_epoch")],
  ["Socket typed ownership forwarding removed", replace(files[13], "joint.gate(rule.rule_id().as_uuid())?", "JointRuleConditionEvaluation::NotOwned")],
  ["actor lifecycle delta removed", replace(files[13], "rule.lifecycle_delta_for_successful_match", "legacy_lifecycle_delta")],
  ["legacy RuleEngine re-enters actor", replace(files[5], "mod evaluation;", "mod evaluation;\nstruct RuleEngine;")],
  ["legacy gate re-enters evaluation", replace(files[13], "use chrono::Utc;", "use chrono::Utc;\nfn evaluate_with_condition_gate_in_order() {}")],
  ["legacy runtime_rules re-enters actor", replace(files[5], "mod evaluation;", "mod evaluation;\nfn runtime_rules() {}")],
  ["legacy lifecycle replacement re-enters evaluation", replace(files[13], "use chrono::Utc;", "use chrono::Utc;\nfn replace_runtime_rule_lifecycle() {}")],
  ["Relay stages collapse to one stage", replace(files[12], "RuleStage::ProxyToApp,", "RuleStage::ProxyToUpstream,")],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    assert.notEqual(run(target).status, 0);
  });
}

test("fails closed when Phase11 Cargo discovery drifts", () => {
  const target = sandbox();
  const result = spawnSync(process.execPath, ["run", "-A", checker], {
    cwd: target,
    encoding: "utf8",
    env: { ...process.env, PHASE11_CHECKER_TEST_MODE: "sandbox", PHASE11_DISCOVERY_COUNT: "0" },
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /expected 1 Phase11 production test/u);
});
