import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase6-contract.mjs");
const discovery = JSON.stringify({
  domain: [
    "draft_rejects_forged_runtime_statistics",
    "draft_rejects_removed_configuration_fields",
    "lifecycle_delta_is_tentative_until_explicitly_applied",
    "save_draft_cannot_supply_runtime_statistics_and_create_initializes_them",
    "successful_match_never_disables_the_rule",
    "update_preserves_runtime_statistics_and_copy_resets_them",
  ],
  infrastructure: ["a::production_http_actor_keeps_rules_enabled_after_commit", "b::socket_encode_failure_rolls_back_lifecycle_before_successful_commit", "c::aborting_http_caller_after_commit_started_does_not_cancel_actor_state_machine"],
});

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase6-current-"));
  for (const relative of ["scripts", "src", "src-tauri"]) {
    fs.cpSync(path.join(root, relative), path.join(target, relative), {
      recursive: true,
      filter: (source) => !source.includes(`${path.sep}target${path.sep}`) && !source.includes(`${path.sep}node_modules${path.sep}`),
    });
  }
  return target;
}

function run(cwd, discovered = discovery) {
  return spawnSync(process.execPath, [checker], { cwd, encoding: "utf8", env: { ...process.env, PHASE6_CHECKER_TEST_MODE: "sandbox", PHASE6_DISCOVERY_JSON: discovered } });
}

function mutate(file, transform) {
  return (target) => {
    const absolute = path.join(target, file);
    fs.writeFileSync(absolute, transform(fs.readFileSync(absolute, "utf8")));
  };
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, change, message] of [
  ["Nth condition", mutate("src-tauri/crates/domain/src/unified_rule_execution.rs", (source) => source.replace("pub enum Condition {", "pub enum Condition { NthHit { count: u64 },")), "Nth"],
  ["multiple conditions", mutate("src-tauri/crates/domain/src/unified_rule.rs", (source) => source.replace("pub condition: Condition,", "pub conditions: Vec<Condition>,")), "condition owner"],
  ["multiple actions", mutate("src-tauri/crates/domain/src/unified_rule.rs", (source) => source.replace("pub action: UnifiedAction,", "pub actions: Vec<UnifiedAction>,")), "action owner"],
  ["parallel transaction", (target) => fs.writeFileSync(path.join(target, "src-tauri/crates/application/src/rule_chain_transaction.rs"), "pub struct RuleChainTransaction;\n"), "parallel RuleChain"],
  ["retry helper", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", (source) => `${source}\nfn legacy_retry() { for _ in 0..=3 {} }\n`), "retry"],
  ["commit removed", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", (source) => source.replace(".commit_runtime_deltas(", ".removed_commit(")), "commit owner"],
  ["successful-hit validation removed", mutate("src-tauri/crates/domain/src/unified_rule/lifecycle.rs", (source) => source.replace("has_hit != self.last_hit_at.is_some()", "false")), "lifecycle validation"],
  ["removed one-shot field restored", mutate("src-tauri/crates/domain/src/unified_rule.rs", (source) => source.replace("pub enabled: bool,", "pub enabled: bool,\n    pub one_shot: bool,")), "removed rule contract returned"],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    change(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(message, "i"));
  });
}

test("zero discovery fails closed", () => {
  const result = run(root, JSON.stringify({ domain: [], infrastructure: [] }));
  assert.notEqual(result.status, 0);
});
