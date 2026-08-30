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
    "common_nth_hit_rejects_zero_at_the_domain_owner",
    "draft_rejects_forged_runtime_statistics",
    "lifecycle_delta_is_tentative_until_explicitly_applied",
    "nth_hit_is_a_common_leaf_and_a_miss_does_not_consume_lifecycle",
    "nth_only_delta_is_valid_for_the_shared_commit_contract_and_does_not_mutate_definition",
    "nth_only_delta_cannot_disable_one_shot_without_a_successful_hit",
    "one_shot_disables_and_advances_revision_only_when_delta_is_applied",
    "save_draft_cannot_supply_runtime_statistics_and_create_initializes_them",
    "update_preserves_runtime_statistics_and_copy_resets_them",
  ],
  application: [
    "commit_conflict_returns_no_partial_output_and_is_not_retried",
    "commit_validation_failure_returns_no_partial_output_and_is_not_retried",
    "concurrent_same_revision_has_one_winner_and_one_single_conflict",
    "condition_action_encode_and_cancel_fail_before_commit",
    "condition_error_preserves_the_complete_application_error",
    "nth_snapshot_is_isolated_by_ip_and_certificate",
    "nth_two_advances_on_successful_miss_then_matches_same_terminal",
    "plan_rejects_mismatched_and_duplicate_rule_owners_before_execution",
    "terminal_is_pending_until_commit_and_stops_lower_rules",
    "terminal_mismatch_fails_before_any_port_or_commit_call",
    "transaction_exposes_prior_mutations_only_after_single_commit",
  ],
  infrastructure: [
    "adapters::listener_runtime::tests::http_protocol_pipeline_tests::revision_conflict_keeps_joint_message_and_lifecycle_uncommitted",
    "adapters::pipeline::tests::aborting_http_caller_after_commit_started_does_not_cancel_actor_state_machine",
    "adapters::pipeline::tests::actor_validation_failure_restores_nth_checkpoint_before_next_evaluation",
    "adapters::pipeline::tests::nth_hit_conflict_is_not_retried_or_consumed",
    "adapters::pipeline::tests::nth_hit_actor_isolates_attempts_by_terminal_ip_and_certificate",
    "adapters::rules::rules_tests::conversion::repository_conversion_rejects_zero_duplicate_oversized_and_wrong_id_deltas",
    "adapters::rules::rules_tests::conversion::repository_conversion_rejects_nth_only_one_shot_disable_without_partial_write",
    "adapters::rules::rules_tests::conversion::runtime_delta_rejects_decrease_instead_of_saturating_it",
  ],
});

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase6-contract-"));
  for (const relative of ["scripts", "src", "src-tauri", "test-support"]) {
    fs.cpSync(path.join(root, relative), path.join(target, relative), {
      recursive: true,
      filter: (source) => !source.endsWith(`${path.sep}target`)
        && !source.includes(`${path.sep}target${path.sep}`)
        && !source.endsWith(`${path.sep}node_modules`)
        && !source.includes(`${path.sep}node_modules${path.sep}`),
    });
  }
  return target;
}

function run(cwd) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE6_CHECKER_TEST_MODE: "sandbox", PHASE6_DISCOVERY_JSON: discovery },
  });
}

function mutate(file, transform) {
  return (target) => {
    const filePath = path.join(target, file);
    fs.writeFileSync(filePath, transform(fs.readFileSync(filePath, "utf8")));
  };
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, change, message] of [
  ["ignored test", mutate("src-tauri/crates/domain/tests/phase6_rule_lifecycle.rs", (s) => s.replace("#[test]", "#[test]\n#[ignore]")), "ignored"],
  ["old nth owner", mutate("src-tauri/crates/domain/src/rule/types.rs", (s) => s.replace("pub enum MatchCondition {", "pub enum MatchCondition { NthHit(u64),")), "NthHit"],
  ["content lifecycle", mutate("src-tauri/crates/domain/src/unified_rule.rs", (s) => s.replace("pub struct HttpRuleContent {", "pub struct HttpRuleContent { pub hit_count: u64,")), "content lifecycle"],
  ["retry loop", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", (s) => `${s}\nfn legacy_retry() { for _ in 0..=3 {} }\n`), "retry"],
  ["renamed retry helper", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", (s) => `${s}\nasync fn again(repo: &dyn RuntimeRuleRepository, base: &RuntimeRuleSnapshot, deltas: &[RuleLifecycleDelta]) { let _ = repo.commit_runtime_deltas(base, deltas).await; }\n`), "retry helper"],
  ["full snapshot port", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline.rs", (s) => s.replace("deltas: &[RuleLifecycleDelta]", "evaluated_rules: &[Rule]")), "delta port"],
  ["direct message leak", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => `${s}\nfn leak(_: &mut intercept_proxy_runtime::Message) {}\n`), "runtime message leak"],
  ["fallback false", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => `${s}\nfn fallback(v: Option<bool>) -> bool { v.unwrap_or(false) }\n`), "fallback"],
  ["public transaction state", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => s.replace("http: Arc<dyn RuleChainHttpPort>", "pub http: Arc<dyn RuleChainHttpPort>")), "private owner"],
  ["transaction alias owner", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => `${s}\ntype ShadowTransaction = RuleChainTransaction;\n`), "alias owner"],
  ["lifecycle delta alias owner", mutate("src-tauri/crates/domain/src/unified_rule.rs", (s) => `${s}\ntype ShadowDelta = RuleLifecycleDelta;\n`), "alias owner"],
  ["lifecycle prewrite", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", (s) => s.replace("match rules.commit_runtime_deltas", "base.rules[0].apply_lifecycle_delta(&deltas[0]).unwrap();\n            match rules.commit_runtime_deltas")), "prewrite"],
  ["precommit output", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => s.replace("let collection_revision = if", "let _precommit = RuleChainOutput { };\n        let collection_revision = if")), "precommit"],
  ["precommit terminal port", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => s.replace("let collection_revision = if", "apply_control(terminal_action.as_ref());\n        let collection_revision = if")), "terminal/control"],
  ["old success retry test", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/tests/rules_and_faults/conflict_no_retry.rs", (s) => `${s}\nfn conflict_retry_succeeds() {}\n`), "success retry"],
  ["empty test", mutate("src-tauri/crates/domain/tests/phase6_rule_lifecycle.rs", (s) => s.replace("fn lifecycle_delta_is_tentative_until_explicitly_applied() {", "fn lifecycle_delta_is_tentative_until_explicitly_applied() {}\nfn removed_body() {")), "empty"],
  ["terminal identity removed", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => s.replace("pub terminal: TerminalIdentity", "pub terminal: String")), "terminal identity"],
  ["tuple rule plan restored", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => s.replace("pub plan: RuleChainPlan", "pub rules: Vec<(RuleProgramEntry, RuleLifecycleSnapshot)>")), "tuple rule plan"],
  ["save statistics bypass", mutate("src-tauri/crates/domain/src/unified_rule.rs", (s) => s.replace("pub struct RuleDefinitionDraft {", "pub struct RuleDefinitionDraft { pub hit_count: u64,")), "runtime statistics"],
  ["HTTP application error downgrade", mutate("src-tauri/crates/application/src/rule_chain_transaction.rs", (s) => s.replace("self.http.matches(&working_message, condition)", "self.http.matches(&working_message, condition).map_err(|_| intercept_proxy_domain::DomainError::new(intercept_proxy_domain::ErrorCode::RuleInvalid, \"downgraded\"))")), "AppError downgrade"],
  ["saturating lifecycle subtraction", mutate("src-tauri/crates/infrastructure/src/adapters/rules/conversion.rs", (s) => s.replace("evaluated.hit_count.checked_sub(original.hit_count)", "Some(evaluated.hit_count.saturating_sub(original.hit_count))")), "saturating"],
  ["hit count restored as Nth owner", mutate("src-tauri/crates/domain/src/unified_rule_execution.rs", (s) => s.replace("nth_attempt == *count", "lifecycle.hit_count == *count")), "hit_count"],
  ["duplicate delta validation removed", mutate("src-tauri/crates/infrastructure/src/adapters/rules/conversion.rs", (s) => s.replace("std::collections::BTreeSet::new()", "std::collections::VecDeque::new()")), "duplicate"],
  ["Nth-only one-shot guard removed", mutate("src-tauri/crates/domain/src/unified_rule/lifecycle.rs", (s) => s.replace("if self.disable_one_shot && !has_hit", "if false")), "one-shot disable guard"],
  ["actor validation rollback removed", mutate("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs", (s) => s.replaceAll("current.engine = checkpoint;", "drop(checkpoint.clone());")), "rollback checkpoint"],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    change(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(message, "i"));
  });
}

test("comments and unrelated retry text remain accepted", () => {
  const target = sandbox();
  const file = path.join(target, "src-tauri/crates/application/src/rule_chain_transaction.rs");
  fs.appendFileSync(file, "\n// retry unwrap_or(false) &mut Message\nconst HELP: &str = \"retry\";\n");
  const result = run(target);
  assert.equal(result.status, 0, result.stderr);
});

test("fake discovery environment is ignored outside checker test mode", () => {
  const result = spawnSync(process.execPath, [checker], {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, PHASE6_DISCOVERY_JSON: JSON.stringify({ domain: [], application: [], infrastructure: [] }) },
  });
  assert.equal(result.status, 0, result.stderr);
});
