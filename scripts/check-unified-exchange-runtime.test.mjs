import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const checker = resolve(root, "scripts/check-unified-exchange-runtime.mjs");

test("current production runtime has one unified working Exchange owner", () => {
  const result = spawnSync(process.execPath, ["run", "-A", checker, root], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
});

function mutated(relativePath, change) {
  const sandbox = mkdtempSync(join(tmpdir(), "unified-exchange-runtime-"));
  try {
    for (const path of [
      "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
      "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs",
      "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline.rs",
      "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/contract.rs",
      "src-tauri/crates/infrastructure/src/adapters/rules.rs",
    ]) {
      const target = resolve(sandbox, path);
      mkdirSync(dirname(target), { recursive: true });
      cpSync(resolve(root, path), target, { recursive: false });
    }
    const target = resolve(sandbox, relativePath);
    writeFileSync(target, change(readFileSync(target, "utf8")));
    return spawnSync(process.execPath, ["run", "-A", checker, sandbox], { encoding: "utf8" });
  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
}

test("legacy actor projection re-entry is rejected", () => {
  const result = mutated(
    "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
    (source) => `${source}\n// RuleEngine workspace.runtime_rules()`,
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /must not split message rules/u);
});

test("runtime snapshot projection re-entry is rejected", () => {
  const result = mutated(
    "src-tauri/crates/infrastructure/src/adapters/rules.rs",
    (source) => source.replace("workspace.rule_definitions.clone()", "workspace.runtime_rules()?"),
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /without a legacy Rule projection/u);
});

test("schema fallback re-entry is rejected", () => {
  const result = mutated(
    "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline.rs",
    (source) => `${source}\n// fallback_schema`,
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /absent schema as None/u);
});
