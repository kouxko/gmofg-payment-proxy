import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { test } from "node:test";
import { dirname, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const checker = resolve(repo, "scripts/check-unified-rule-model.mjs");

test("current Rust source has one unified rule model", () => {
  const result = spawnSync(process.execPath, ["run", "-A", checker, repo], {
    cwd: repo,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
});

test("checker rejects reintroducing a legacy rule model", () => {
  const root = mkdtempSync(resolve(tmpdir(), "unified-rule-model-"));
  try {
    for (const directory of ["src-tauri/crates/domain/src", "src-tauri/src", "test-support"]) {
      mkdirSync(resolve(root, directory), { recursive: true });
    }
    writeFileSync(
      resolve(root, "src-tauri/crates/domain/src/lib.rs"),
      "pub struct ProtocolDocumentRuleDefinition;\n",
    );
    const result = spawnSync(process.execPath, ["run", "-A", checker, root], {
      cwd: root,
      encoding: "utf8",
    });
    assert.notEqual(result.status, 0, result.stdout);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects a second Rule and RuleEngine aggregate", () => {
  const root = mkdtempSync(resolve(tmpdir(), "unified-rule-model-"));
  try {
    const engine = resolve(root, "src-tauri/crates/domain/src/rule/engine.rs");
    mkdirSync(dirname(engine), { recursive: true });
    mkdirSync(resolve(root, "src-tauri/src"), { recursive: true });
    mkdirSync(resolve(root, "test-support"), { recursive: true });
    writeFileSync(engine, "pub struct RuleEngine;\npub struct RuleDraft;\n");
    const result = spawnSync(process.execPath, ["run", "-A", checker, root], {
      cwd: root,
      encoding: "utf8",
    });
    assert.notEqual(result.status, 0, result.stdout);
    assert.match(result.stderr, /second rule model owner|RuleEngine/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects reintroducing only a public Rule aggregate", () => {
  const root = mkdtempSync(resolve(tmpdir(), "unified-rule-model-"));
  try {
    const model = resolve(root, "src-tauri/crates/domain/src/alternate.rs");
    mkdirSync(dirname(model), { recursive: true });
    mkdirSync(resolve(root, "src-tauri/src"), { recursive: true });
    mkdirSync(resolve(root, "test-support"), { recursive: true });
    writeFileSync(model, "pub struct Rule;\n");
    const result = spawnSync(process.execPath, ["run", "-A", checker, root], {
      cwd: root,
      encoding: "utf8",
    });
    assert.notEqual(result.status, 0, result.stdout);
    assert.match(result.stderr, /pub\\s\+.*Rule/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects reintroducing a recursive ConditionTree aggregate", () => {
  const root = mkdtempSync(resolve(tmpdir(), "unified-rule-model-"));
  try {
    const model = resolve(root, "src-tauri/crates/domain/src/alternate.rs");
    mkdirSync(dirname(model), { recursive: true });
    mkdirSync(resolve(root, "src-tauri/src"), { recursive: true });
    mkdirSync(resolve(root, "test-support"), { recursive: true });
    writeFileSync(model, "pub enum ConditionTree { All(Vec<ConditionTree>) }\n");
    const result = spawnSync(process.execPath, ["run", "-A", checker, root], {
      cwd: root,
      encoding: "utf8",
    });
    assert.notEqual(result.status, 0, result.stdout);
    assert.match(result.stderr, /ConditionTree/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects reintroducing the removed TLS rule stage or action", () => {
  for (const source of [
    "let stage = RuleStage::TlsHandshake;\n",
    "let action = TerminalAction::RejectTlsHandshake;\n",
  ]) {
    const root = mkdtempSync(resolve(tmpdir(), "unified-rule-model-"));
    try {
      const model = resolve(root, "src-tauri/crates/domain/src/alternate.rs");
      mkdirSync(dirname(model), { recursive: true });
      mkdirSync(resolve(root, "src-tauri/src"), { recursive: true });
      mkdirSync(resolve(root, "test-support"), { recursive: true });
      writeFileSync(model, source);
      const result = spawnSync(process.execPath, ["run", "-A", checker, root], {
        cwd: root,
        encoding: "utf8",
      });
      assert.notEqual(result.status, 0, result.stdout);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects a legacy HTTP condition projection helper", () => {
  const root = mkdtempSync(resolve(tmpdir(), "unified-rule-model-"));
  try {
    const model = resolve(root, "src-tauri/crates/domain/src/condition_tree.rs");
    mkdirSync(dirname(model), { recursive: true });
    mkdirSync(resolve(root, "src-tauri/src"), { recursive: true });
    mkdirSync(resolve(root, "test-support"), { recursive: true });
    writeFileSync(model, "pub fn from_http_conditions() {}\n");
    const result = spawnSync(process.execPath, ["run", "-A", checker, root], {
      cwd: root,
      encoding: "utf8",
    });
    assert.notEqual(result.status, 0, result.stdout);
    assert.match(result.stderr, /from_http_conditions/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
