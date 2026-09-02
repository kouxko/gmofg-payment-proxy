import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const checker = resolve(import.meta.dirname, "check-task-20260829-002-phase14-schema100.mjs");
const repo = resolve(import.meta.dirname, "..");

function fixture(mutator) {
  const root = mkdtempSync(join(tmpdir(), "phase14-schema100-"));
  for (const relative of [
    "src-tauri/crates/infrastructure/src/sqlite/schema.rs",
    "src-tauri/crates/infrastructure/src/sqlite/external_packages.rs",
    "src-tauri/crates/infrastructure/src/sqlite/environment_configuration_baseline.rs",
    "src-tauri/crates/infrastructure/src/sqlite/workspaces.rs",
  ]) {
    const target = join(root, relative);
    mkdirSync(resolve(target, ".."), { recursive: true });
    writeFileSync(target, readFileSync(join(repo, relative), "utf8"));
  }
  mutator?.(root);
  return root;
}

function run(root) {
  return spawnSync(process.execPath, ["run", "-A", checker, root], {
    cwd: root,
    encoding: "utf8",
  });
}

test("current source satisfies the final Schema100 contract", () => {
  const result = run(repo);
  assert.equal(result.status, 0, result.stderr);
});

test("checker rejects either legacy package table", () => {
  for (const table of ["protocol_packages", "protocol_package_files"]) {
    const root = fixture((directory) => {
      const path = join(directory, "src-tauri/crates/infrastructure/src/sqlite/schema.rs");
      writeFileSync(path, `${readFileSync(path, "utf8")}\n-- SELECT * FROM ${table};\n`);
    });
    try {
      assert.notEqual(run(root).status, 0, table);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects the retired Schema19 proxy-rules validation path", () => {
  const root = fixture((directory) => {
    const path = join(directory, "scripts/e2e_proxy_rules.py");
    mkdirSync(resolve(path, ".."), { recursive: true });
    writeFileSync(path, "SELECT enabled FROM protocol_packages\n");
  });
  try {
    const result = run(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /obsolete Schema19 validation path remains/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
