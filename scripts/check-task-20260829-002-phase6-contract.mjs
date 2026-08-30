import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const fail = (message) => { throw new Error(`TASK-20260829-002 Phase6 contract: ${message}`); };
const stripComments = (source) => source
  .replace(/\/\*[\s\S]*?\*\//gu, " ")
  .replace(/\/\/[^\n]*/gu, " ");
const codeOnly = (source) => stripComments(source)
  .replace(/r#*"[\s\S]*?"#*/gu, '""')
  .replace(/"(?:\\.|[^"\\])*"/gu, '""')
  .replace(/'(?:\\.|[^'\\])*'/gu, "''");

function walk(directory) {
  const files = [];
  for (const entry of fs.readdirSync(path.join(root, directory), { withFileTypes: true })) {
    const relative = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...walk(relative));
    else if (relative.endsWith(".rs")) files.push(relative);
  }
  return files;
}

function command(args) {
  const result = spawnSync("cargo", args, { cwd: root, encoding: "utf8" });
  if (result.status !== 0) fail(`test discovery failed: ${result.stderr}`);
  return result.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6));
}

function discover() {
  if (process.env.PHASE6_CHECKER_TEST_MODE === "sandbox") return JSON.parse(process.env.PHASE6_DISCOVERY_JSON);
  const base = ["test", "--manifest-path", "src-tauri/Cargo.toml"];
  return {
    domain: command([...base, "-p", "intercept-proxy-domain", "--test", "phase6_rule_lifecycle", "--", "--list", "--format", "terse"]),
    application: [
      ...command([...base, "-p", "intercept-proxy-application", "--test", "phase6_rule_chain_transaction", "--", "--list", "--format", "terse"]),
      ...command([...base, "-p", "intercept-proxy-application", "--test", "phase6_review_repairs", "--", "--list", "--format", "terse"]),
    ],
    infrastructure: command([...base, "-p", "intercept-proxy-infrastructure", "--lib", "--", "--list", "--format", "terse"]).filter((line) => /nth_hit_(?:conflict|actor_isolates)|actor_validation_failure_restores|revision_conflict_keeps_joint|aborting_http_caller|runtime_delta_rejects_decrease|repository_conversion_rejects/u.test(line)),
  };
}

const inventory = JSON.parse(read("test-support/fixtures/task-20260829-002/phase-6/rule-chain-transaction/contract-inventory.json"));

const testFiles = [
  "src-tauri/crates/domain/tests/phase6_rule_lifecycle.rs",
  "src-tauri/crates/application/tests/phase6_rule_chain_transaction.rs",
  "src-tauri/crates/application/tests/phase6_review_repairs.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/tests/rules_and_faults/conflict_no_retry.rs",
  "src-tauri/crates/infrastructure/src/adapters/listener_runtime/tests/http_protocol_pipeline/joint_atomic.rs",
  "src-tauri/crates/infrastructure/src/adapters/pipeline/tests/rules_and_faults.rs",
];
const tests = testFiles.map(read).join("\n");
if (/#\[(?:ignore|should_panic)\]|\.(?:skip|only)\s*\(/u.test(stripComments(tests))) fail("ignored, skipped, or panic-only Phase6 test");
if (/#[^\n]*test[^\n]*\]\s*(?:async\s+)?fn\s+\w+\s*\([^)]*\)\s*\{\s*\}/u.test(codeOnly(tests))) fail("empty Phase6 test detected");
const discovered = discover();
for (const [kind, expected] of Object.entries(inventory.discoverable_tests)) {
  const actual = [...discovered[kind]].sort();
  const canonical = [...expected].sort();
  if (actual.length === 0 || JSON.stringify(actual) !== JSON.stringify(canonical)) fail(`Cargo discovered ${kind} tests drift: expected=${JSON.stringify(canonical)}, actual=${JSON.stringify(actual)}`);
}

const production = [
  ...walk("src-tauri/crates/domain/src"),
  ...walk("src-tauri/crates/application/src"),
  ...walk("src-tauri/crates/infrastructure/src").filter((file) => !file.includes("/tests/")),
  ...walk("src-tauri/src").filter((file) => !file.includes("/tests/")),
];
const allCode = production.map((file) => codeOnly(read(file))).join("\n");
for (const symbol of ["RuleLifecycle", "RuleLifecycleSnapshot", "RuleLifecycleDelta", "RuleChainTransaction"]) {
  const count = [...allCode.matchAll(new RegExp(`\\bstruct\\s+${symbol}\\b`, "gu"))].length;
  if (count !== 1) fail(`single owner drift for ${symbol}: ${count}`);
  if (new RegExp(`\\btype\\s+\\w+\\s*=\\s*${symbol}\\b`, "u").test(allCode)) fail(`alias owner drift for ${symbol}`);
}

const unified = codeOnly(read("src-tauri/crates/domain/src/unified_rule.rs"));
for (const symbol of ["HttpRuleContent", "SocketRuleContent"]) {
  const body = unified.match(new RegExp(`struct\\s+${symbol}\\s*\\{([\\s\\S]*?)\\n\\}`, "u"))?.[1] ?? "";
  if (/\b(?:one_shot|hit_count|last_hit_at)\b/u.test(body)) fail(`content lifecycle owner detected in ${symbol}`);
}
const ruleTypes = codeOnly(read("src-tauri/crates/domain/src/rule/types.rs"));
const matchBody = ruleTypes.match(/enum\s+MatchCondition\s*\{([\s\S]*?)\n\}/u)?.[1] ?? "";
if (/\bNthHit\b/u.test(matchBody)) fail("NthHit remains owned by MatchCondition");

const transaction = codeOnly(read("src-tauri/crates/application/src/rule_chain_transaction.rs"));
const ownerBody = transaction.match(/struct\s+RuleChainTransaction\s*\{([\s\S]*?)\n\}/u)?.[1] ?? "";
if (/\bpub\s+(?:\([^)]*\)\s+)?\w+\s*:/u.test(ownerBody)) fail("RuleChainTransaction private owner state leaked");
if (/&\s*mut\s+(?:[\w:]+::)?(?:Message|Exchange)\b/u.test(transaction)) fail("runtime message leak into Application transaction");
if (/\.unwrap_or(?:_else)?\s*\(\s*(?:false|Default::default|\w+::default)/u.test(transaction)) fail("fallback false/default in transaction");
if (!/terminal:\s*TerminalIdentity/u.test(transaction)) fail("typed terminal identity missing from transaction input");
if (/rules:\s*Vec\s*<\s*\(\s*RuleProgramEntry/u.test(transaction)) fail("public tuple rule plan detected");
for (const symbol of ["RuleChainPlan", "RuleChainPlanEntry"]) {
  const body = transaction.match(new RegExp(`struct\\s+${symbol}\\s*\\{([\\s\\S]*?)\\n\\}`, "u"))?.[1] ?? "";
  if (/\bpub\s+\w+\s*:/u.test(body)) fail(`${symbol} private owner state leaked`);
}
if (/http\.matches[\s\S]{0,240}DomainError::new[\s\S]{0,120}RuleInvalid/u.test(transaction)) fail("HTTP AppError downgrade detected");
const execute = transaction.slice(transaction.indexOf("pub async fn execute_cancellable"));
const commitAt = execute.indexOf(".commit(");
const outputAt = execute.indexOf("RuleChainOutput {");
if (commitAt < 0 || outputAt < 0 || outputAt < commitAt) fail("precommit output/control publication detected");
if (/\b(?:publish|emit|send|dispatch|apply_control)\s*\([^)]*(?:terminal|control)/u.test(execute.slice(0, commitAt))) fail("precommit terminal/control port call detected");

const actor = codeOnly(read("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs"));
if (/0\s*\.\.\s*=\s*3|remaining_retries|legacy_retry|\bcontinue\s*;/u.test(actor)) fail("retry loop/helper detected in rule actor");
if ((actor.match(/\.commit_runtime_deltas\s*\(/gu) ?? []).length !== 1) fail("renamed or aliased retry helper detected in rule actor");
if (/\.apply_lifecycle_delta\s*\(/u.test(actor.slice(0, actor.indexOf(".commit_runtime_deltas(")))) fail("lifecycle prewrite detected before atomic commit");
if (!/nth_counter_snapshots/u.test(actor) || !/!nth_advances\.is_empty/u.test(actor)) fail("Nth counter transaction commit owner missing");
if ((actor.match(/current\.engine\s*=\s*checkpoint/gu) ?? []).length < 4) fail("actor validation rollback checkpoint missing");
const conversion = codeOnly(read("src-tauri/crates/infrastructure/src/adapters/rules/conversion.rs"));
if (/saturating_sub/u.test(conversion)) fail("saturating lifecycle subtraction detected");
if (!/BTreeSet/u.test(conversion)) fail("delta duplicate validation missing");
if (!/\.validate_against\s*\(/u.test(conversion)) fail("adapter bypasses shared lifecycle validation owner");
const lifecycle = codeOnly(read("src-tauri/crates/domain/src/unified_rule/lifecycle.rs"));
if (!/increment\s*!=\s*1/u.test(lifecycle)) fail("exact lifecycle increment validation missing");
if (!/disable_one_shot\s*&&\s*!has_hit/u.test(lifecycle)) fail("Nth-only one-shot disable guard missing");
const execution = codeOnly(read("src-tauri/crates/domain/src/unified_rule_execution.rs"));
if (/NthHit[\s\S]{0,180}hit_count/u.test(execution)) fail("hit_count used as Nth attempt owner");
const draftBody = unified.match(/struct\s+RuleDefinitionDraft\s*\{([\s\S]*?)\n\}/u)?.[1] ?? "";
if (/\b(?:lifecycle|hit_count|last_hit_at|revision)\b/u.test(draftBody)) fail("save draft exposes runtime statistics");
const pipeline = codeOnly(read("src-tauri/crates/infrastructure/src/adapters/pipeline.rs"));
const port = pipeline.match(/trait\s+RuntimeRuleRepository[\s\S]*?\n\}/u)?.[0] ?? "";
if (!/commit_runtime_deltas[\s\S]*RuleLifecycleDelta/u.test(port) || /evaluated_rules|\[\s*Rule\s*\]/u.test(port)) fail("delta port widened to full evaluated rules");
if (/commit_runtime_snapshot/u.test(allCode)) fail("old full snapshot commit owner remains");

const retryTests = codeOnly(testFiles.filter((file) => file.includes("infrastructure")).map(read).join("\n"));
if (/conflict_retry|retry[_\s].*succeed|succeed.*retry/iu.test(retryTests)) fail("old success retry test detected");

console.log(`TASK-20260829-002 Phase6 contract PASS (Domain=${discovered.domain.length}, Application=${discovered.application.length}, Infrastructure=${discovered.infrastructure.length})`);
