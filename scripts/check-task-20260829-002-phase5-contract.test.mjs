import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase5-contract.mjs");
const inventory = JSON.parse(fs.readFileSync(path.join(
  root,
  "test-support/fixtures/task-20260829-002/phase-5/unified-rule-domain/contract-inventory.json",
), "utf8"));
const sandboxDiscovery = JSON.stringify({
  rust: inventory.discoverable_tests.rust_names,
  typescript: inventory.discoverable_tests.typescript_names,
});

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase5-contract-"));
  for (const relative of ["scripts", "src", "src-tauri", "test-support"]) {
    fs.cpSync(path.join(root, relative), path.join(target, relative), {
      recursive: true,
      filter: (source) => !source.includes(`${path.sep}target${path.sep}`)
        && !source.includes(`${path.sep}node_modules${path.sep}`),
    });
  }
  return target;
}

function run(cwd) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE5_DISCOVERY_JSON: sandboxDiscovery },
  });
}

function append(relative, contents) {
  return (target) => {
    const filePath = path.join(target, relative);
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.appendFileSync(filePath, contents);
  };
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, file, mutate, message] of [
  ["created_order comparator", "src/features/rules/rule-definition-model.ts", (s) => s.replace("left.priority - right.priority || left.rule_id.localeCompare(right.rule_id)", "left.priority - right.priority || left.created_order - right.created_order"), "created_order"],
  ["generated discriminator", "src/generated/rust-types.ts", (s) => s.replace('operator: "all"', 'operator: "all_drift"'), "generated semantic drift"],
  ["flat condition owner", "src-tauri/crates/domain/src/unified_rule.rs", (s) => `${s}\npub struct Bad { pub conditions: Vec<MatchCondition> }\n`, "flat condition owner"],
  ["parallel action owner", "src-tauri/crates/domain/src/unified_rule.rs", (s) => `${s}\npub struct Bad { pub actions: Vec<RuleAction> }\n`, "parallel action owner"],
  ["ignored test", "src-tauri/crates/domain/tests/phase5_unified_rule_domain.rs", (s) => s.replace("#[test]", "#[test]\n#[ignore]"), "ignored"],
  ["stale allowlist", "src-tauri/crates/domain/src/rule/types.rs", () => "", "stale Phase12 allowlist"],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    const filePath = path.join(target, file);
    fs.writeFileSync(filePath, mutate(fs.readFileSync(filePath, "utf8")));
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(message));
  });
}

test("comments and unrelated aliases do not create false owners", () => {
  const target = sandbox();
  const filePath = path.join(target, "src-tauri/crates/domain/src/unified_rule.rs");
  fs.appendFileSync(filePath, "\n// pub conditions: Vec<MatchCondition>\ntype HistoricalLabel = String;\n");
  const result = run(target);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, mutate, message] of [
  ["created_order comparator in an unlisted helper", append(
    "src-tauri/crates/domain/src/phase5_shadow_sort.rs",
    "\npub fn sort(rules: &mut [Rule]) { rules.sort_by_key(|rule| (rule.priority, rule.created_order, rule.id)); }\n",
  ), "created_order"],
  ["second serde wire owner", append(
    "src-tauri/crates/domain/src/unified_rule_execution.rs",
    "\n#[derive(Serialize, Deserialize, Type)]\n#[serde(tag = \"source\", content = \"value\", rename_all = \"snake_case\")]\npub enum ShadowUnifiedAction { RecordMatch, Document(DocumentMutation), Http(RuleAction), Terminal(TerminalAction) }\n",
  ), "second unified wire owner"],
  ["generated extra variant", (target) => {
    const filePath = path.join(target, "src/generated/rust-types.ts");
    fs.writeFileSync(filePath, fs.readFileSync(filePath, "utf8").replace(
      '{ source: "terminal"; value: TerminalAction };',
      '{ source: "terminal"; value: TerminalAction } |\n{ source: "legacy_extra" };',
    ));
  }, "generated semantic drift"],
  ["extra legacy owner inside an allowlisted file", append(
    "src-tauri/crates/domain/src/rule/types.rs",
    "\n#[derive(Clone)]\npub enum RuleAction { Shadow }\n",
  ), "legacy owner count"],
  ["comment-only Rust test count", (target) => {
    const filePath = path.join(target, "src-tauri/crates/domain/tests/phase5_unified_rule_domain.rs");
    fs.writeFileSync(filePath, `/*
#[test]
fn fake_one() {}
#[test]
fn fake_two() {}
#[test]
fn fake_three() {}
#[test]
fn fake_four() {}
#[test]
fn fake_five() {}
#[test]
fn fake_six() {}
#[test]
fn fake_seven() {}
*/\n`);
  }, "Cargo discovered"],
  ["alias second owner", append(
    "src-tauri/crates/domain/src/unified_rule_execution.rs",
    "\npub type SecondaryUnifiedAction = UnifiedAction;\n",
  ), "alias unified wire owner"],
]) {
  test(`fails closed for review mutation: ${name}`, () => {
    const target = sandbox();
    mutate(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, new RegExp(message));
  });
}

test("harmless sort helpers, non-wire enums, and unrelated aliases remain accepted", () => {
  const target = sandbox();
  append(
    "src-tauri/crates/domain/src/phase5_harmless.rs",
    "\npub fn sort(items: &mut [Item]) { items.sort_by_key(|item| item.timestamp); }\npub enum DisplayAction { RecordMatch, Document, Http, Terminal }\npub type DisplayLabel = String;\n",
  )(target);
  const result = run(target);
  assert.equal(result.status, 0, result.stderr);
});
