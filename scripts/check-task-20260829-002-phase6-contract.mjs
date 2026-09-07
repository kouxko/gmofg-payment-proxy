import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const fail = (message) => { throw new Error(`TASK-20260829-002 Phase6 contract: ${message}`); };
const stripComments = (source) => source.replace(/\/\*[\s\S]*?\*\//gu, " ").replace(/\/\/[^\n]*/gu, " ");
const codeOnly = (source) => stripComments(source)
  .replace(/r#*"[\s\S]*?"#*/gu, '""')
  .replace(/"(?:\\.|[^"\\])*"/gu, '""')
  .replace(/'(?:\\.|[^'\\])*'/gu, "''");

function walk(directory) {
  const absolute = path.join(root, directory);
  if (!fs.existsSync(absolute)) return [];
  const files = [];
  for (const entry of fs.readdirSync(absolute, { withFileTypes: true })) {
    const relative = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...walk(relative));
    else files.push(relative);
  }
  return files;
}

function cargoTests(args) {
  const result = spawnSync("cargo", args, { cwd: root, encoding: "utf8" });
  if (result.status !== 0) fail(`test discovery failed: ${result.stderr}`);
  return result.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6));
}

function discovery() {
  if (process.env.PHASE6_CHECKER_TEST_MODE === "sandbox") return JSON.parse(process.env.PHASE6_DISCOVERY_JSON);
  const base = ["test", "--manifest-path", "src-tauri/Cargo.toml"];
  const infrastructure = cargoTests([...base, "-p", "intercept-proxy-infrastructure", "--lib", "--", "--list", "--format", "terse"])
    .filter((name) => [
      "production_http_actor_keeps_rules_enabled_after_commit",
      "socket_encode_failure_rolls_back_lifecycle_before_successful_commit",
      "aborting_http_caller_after_commit_started_does_not_cancel_actor_state_machine",
    ].some((suffix) => name.endsWith(suffix)));
  return {
    domain: cargoTests([...base, "-p", "intercept-proxy-domain", "--test", "phase6_rule_lifecycle", "--", "--list", "--format", "terse"]),
    infrastructure,
  };
}

const discovered = discovery();
const expectedDomain = [
  "draft_rejects_forged_runtime_statistics",
  "draft_rejects_removed_configuration_fields",
  "lifecycle_delta_is_tentative_until_explicitly_applied",
  "save_draft_cannot_supply_runtime_statistics_and_create_initializes_them",
  "successful_match_never_disables_the_rule",
  "update_preserves_runtime_statistics_and_copy_resets_them",
];
if (JSON.stringify([...discovered.domain].sort()) !== JSON.stringify(expectedDomain)) fail(`Domain discovery drift: ${JSON.stringify(discovered.domain)}`);
if (discovered.infrastructure.length !== 3) fail(`Infrastructure discovery drift: ${JSON.stringify(discovered.infrastructure)}`);

const ruleOwner = codeOnly(read("src-tauri/crates/domain/src/unified_rule.rs"));
if (!/pub\s+condition:\s*Condition/u.test(ruleOwner) || /pub\s+conditions:/u.test(ruleOwner)) fail("exactly-one condition owner missing");
if (!/pub\s+action:\s*UnifiedAction/u.test(ruleOwner) || /pub\s+actions:/u.test(ruleOwner)) fail("exactly-one action owner missing");

const productionFiles = [
  ...walk("src-tauri/crates").filter((file) => file.includes("/src/") && file.endsWith(".rs") && !file.includes("/tests/")),
  ...walk("src-tauri/src").filter((file) => file.endsWith(".rs") && !file.includes("/tests/")),
  ...walk("src/features").filter((file) => /\.(?:ts|tsx)$/u.test(file) && !file.includes(".test.")),
  "src/generated/rust-types.ts",
].filter((file) => fs.existsSync(path.join(root, file)));
const production = productionFiles.map((file) => codeOnly(read(file))).join("\n");
for (const token of ["NthHit", "NthCounter", "nth_hit", "ruleDefinitionNthHitConditionDraft", ["one", "_shot"].join(""), "OneShot", "defaultOneShot"]) {
  if (production.includes(token)) fail(`removed rule contract returned: ${token}`);
}
if (fs.existsSync(path.join(root, "src-tauri/crates/application/src/rule_chain_transaction.rs"))) fail("unused parallel RuleChainTransaction file returned");
const applicationLib = codeOnly(read("src-tauri/crates/application/src/lib.rs"));
if (/rule_chain_transaction|RuleChainTransaction/u.test(applicationLib)) fail("unused parallel RuleChainTransaction export returned");

const actor = codeOnly(read("src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs"));
if (!/let\s+checkpoint\s*=\s*current\.clone\(\)/u.test(actor)) fail("actor rollback checkpoint missing");
if ((actor.match(/\.commit_runtime_deltas\s*\(/gu) ?? []).length !== 1) fail("single lifecycle commit owner missing");
if (/0\s*\.\.\s*=\s*3|remaining_retries|legacy_retry/u.test(actor)) fail("runtime retry path returned");

const lifecycle = codeOnly(read("src-tauri/crates/domain/src/unified_rule/lifecycle.rs"));
for (const symbol of ["RuleLifecycle", "RuleLifecycleSnapshot", "RuleLifecycleDelta"]) {
  const count = [...lifecycle.matchAll(new RegExp(`\\bstruct\\s+${symbol}\\b`, "gu"))].length;
  if (count !== 1) fail(`single lifecycle owner drift for ${symbol}: ${count}`);
}
if (!/has_hit\s*!=\s*self\.last_hit_at\.is_some\(\)/u.test(lifecycle)) fail("successful-hit lifecycle validation missing");
if (!/candidate\.lifecycle\.hit_count[\s\S]*checked_add\(delta\.hit_count_increment\)/u.test(lifecycle)) fail("successful-hit counter commit missing");

console.log(`TASK-20260829-002 Phase6 contract PASS (Domain=${discovered.domain.length}, Infrastructure=${discovered.infrastructure.length})`);
