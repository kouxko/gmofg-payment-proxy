import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const failures = [];
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const walk = (directory) => fs.readdirSync(path.join(root, directory), { withFileTypes: true }).flatMap((entry) => {
  const relative = path.join(directory, entry.name);
  return entry.isDirectory() ? walk(relative) : [relative];
});

const activeFiles = ["src", "src-tauri"]
  .flatMap(walk)
  .filter((file) => /\.(?:rs|ts|tsx|json)$/u.test(file))
  .filter((file) => !file.endsWith("phase12_legacy_stage_deletion.rs"));
for (const file of activeFiles) {
  const source = read(file);
  if (/\b(?:AppToProxy|UpstreamToProxy)\b|["'](?:app_to_proxy|upstream_to_proxy)["']/u.test(source)) {
    failures.push(`legacy four-stage contract remains in ${file}`);
  }
  if (/\b(?:first_rules|second_rules|app_to_proxy|upstream_to_proxy)\s*:/u.test(source)) {
    failures.push(`legacy four-stage runtime factory remains in ${file}`);
  }
}

const domainFiles = walk("src-tauri/crates/domain/src").filter((file) => file.endsWith(".rs"));
for (const file of domainFiles) {
  const source = read(file);
  for (const symbol of [
    "DocumentCondition",
    "DocumentAction",
    "MatchCondition",
    "RuleAction",
    "HttpCondition",
  ]) {
    if (new RegExp(`\\b(?:struct|enum|type)\\s+${symbol}\\b`, "u").test(source)) {
      failures.push(`legacy runtime owner ${symbol} remains in ${file}`);
    }
  }
}

const unifiedExecution = read("src-tauri/crates/domain/src/unified_rule_execution.rs");
if (!/Http\s*\{[\s\S]{0,300}?field:\s*MatchField,[\s\S]{0,300}?operator:\s*MatchOperator/u.test(
  unifiedExecution,
)) {
  failures.push("unified Condition must directly own the HTTP field/operator payload");
}

const phase5 = JSON.parse(read("test-support/fixtures/task-20260829-002/phase-5/unified-rule-domain/contract-inventory.json"));
if (!Array.isArray(phase5.phase12_legacy_owner_allowlist)
    || phase5.phase12_legacy_owner_allowlist.length !== 0) {
  failures.push("Phase12 legacy owner allowlist must be empty");
}

const phase1Inventory = read(
  "test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json",
);
if (/four-stage|app_to_proxy|upstream_to_proxy|AppToProxy|UpstreamToProxy/u.test(phase1Inventory)) {
  failures.push("active Phase1 current-state inventory restored a legacy four-stage contract");
}
for (const id of [
  "frontend-two-stage-model-tests",
  "two-stage-document-rules",
  "generated-two-stage-types",
]) {
  if (!phase1Inventory.includes(`\"id\": \"${id}\"`)) {
    failures.push(`active Phase1 current-state inventory is missing ${id}`);
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL: ${failure}`);
  process.exit(1);
}
console.log("PASS: Phase 12 legacy runtime and stage owners deleted");
