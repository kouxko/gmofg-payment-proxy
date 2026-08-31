import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase12-legacy-deletion.mjs");
const paths = {
  unifiedRule: "src-tauri/crates/domain/src/unified_rule.rs",
  ruleTypes: "src-tauri/crates/domain/src/rule/types.rs",
  unifiedExecution: "src-tauri/crates/domain/src/unified_rule_execution.rs",
  externalRelay: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay.rs",
  uiModel: "src/features/rules/rule-definition-model.ts",
  generated: "src/generated/rust-types.ts",
  phase5Inventory: "test-support/fixtures/task-20260829-002/phase-5/unified-rule-domain/contract-inventory.json",
  phase1Inventory: "test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json",
};
const files = Object.values(paths);
function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase12-deletion-"));
  for (const file of files) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  return target;
}
function run(cwd) {
  return spawnSync(process.execPath, [checker], { cwd, encoding: "utf8" });
}
function append(file, text) {
  return (target) => fs.appendFileSync(path.join(target, file), text);
}

test("canonical repository passes", () => assert.equal(run(root).status, 0));
for (const [name, mutate] of [
  ["unified legacy enum", append(paths.unifiedRule, "\nenum Restored { AppToProxy }\n")],
  ["four-stage factory", append(paths.externalRelay, "\nstruct Restored { first_rules: usize }\n")],
  ["legacy UI copy", append(paths.uiModel, "\nconst restored = 'app_to_proxy';\n")],
  ["generated legacy wire", append(paths.generated, "\nexport type Restored = \"upstream_to_proxy\";\n")],
  ["legacy runtime owner", append(paths.unifiedRule, "\npub struct DocumentCondition;\n")],
  ["renamed HTTP condition owner", append(paths.ruleTypes, "\npub enum HttpCondition {}\n")],
  ["stale active Phase1 inventory", append(paths.phase1Inventory, "\napp_to_proxy\n")],
  ["allowlist restored", (target) => {
    const file = path.join(target, paths.phase5Inventory);
    const value = JSON.parse(fs.readFileSync(file, "utf8"));
    value.phase12_legacy_owner_allowlist = [{ file: "x.rs", symbol: "RuleAction", reason: "legacy" }];
    fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
  }],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    assert.notEqual(run(target).status, 0);
  });
}
