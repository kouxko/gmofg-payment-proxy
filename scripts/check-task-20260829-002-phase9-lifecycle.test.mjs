import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase9-lifecycle.mjs");
const files = [
  "src-tauri/crates/package-runtime/src/bin/intercept-proxy-package-sidecar.rs",
  "src-tauri/crates/package-runtime/tests/phase9_sidecar_process.rs",
  "src-tauri/crates/infrastructure/src/adapters/local_package_supervisor.rs",
  "src-tauri/crates/infrastructure/src/adapters/local_package_supervisor/tests.rs",
  "src-tauri/crates/infrastructure/src/adapters/protocol_package_import.rs",
  "src-tauri/crates/infrastructure/src/adapters/protocol_package_import/tests.rs",
  "src-tauri/crates/infrastructure/src/adapters/external_package_registry/mod.rs",
  "src-tauri/crates/infrastructure/src/adapters/external_package_registry/application_port.rs",
  "src-tauri/crates/infrastructure/src/adapters/external_package_server.rs",
  "src-tauri/crates/infrastructure/src/adapters/bundle.rs",
  "src-tauri/crates/infrastructure/src/sqlite/schema.rs",
  "src-tauri/crates/infrastructure/src/sqlite/external_packages.rs",
  "src-tauri/crates/application/src/facade/protocol_packages.rs",
  "src-tauri/crates/application/src/facade/protocol_packages/lifecycle.rs",
  "src-tauri/crates/infrastructure/src/adapters/external_package_registry/local_archives.rs",
  "src-tauri/crates/application/src/ports/external_packages.rs",
  "src-tauri/src/commands/protocol_packages.rs",
  "src/generated/rust-types.ts",
];

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase9-lifecycle-"));
  for (const file of files) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  return target;
}

function run(cwd) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE9_CHECKER_TEST_MODE: "sandbox" },
  });
}

function replace(file, before, after) {
  return (target) => {
    const name = path.join(target, file);
    fs.writeFileSync(name, fs.readFileSync(name, "utf8").split(before).join(after));
  };
}

function append(file, text) {
  return (target) => fs.appendFileSync(path.join(target, file), `\n${text}\n`);
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, mutate, expected] of [
  ["missing package initiated connect", replace(files[0], "connect_async", "accept_async"), /process bridge/u],
  ["missing registration deadline", replace(files[2], "Duration::from_secs(10)", "Duration::from_secs(11)"), /lifecycle/u],
  ["retry policy added", append(files[2], "fn retry_with_backoff() {}"), /must not add retry/u],
  ["restart skips registry disconnect", replace(files[2], "self.registry.disconnect", "self.registry.is_online"), /lifecycle/u],
  ["importer takes Boa ownership", append(files[4], "fn wrong() { LocalSidecarRuntime::load(); }"), /must not own/u],
  ["archive not persisted", replace(files[10], "local_archive BLOB NULL", "local_archive_removed BLOB NULL"), /persist/u],
  ["startup skips enabled packages", replace(files[9], "supervisor.start_enabled", "supervisor.skip_enabled"), /app-start/u],
  ["shutdown leaves children", replace(files[8], "supervisor.shutdown().await", "supervisor.cancel()"), /reap/u],
  ["disabled listener gate removed", replace(files[12], "PROTOCOL_PACKAGE_DISABLED", "PACKAGE_MAY_RUN"), /disabled/u],
  ["exact lifecycle gate removed", replace(files[2], "lifecycle_gate", "unserialized_lifecycle"), /lifecycle/u],
  ["manual restart port removed", replace(files[15], "async fn restart", "async fn relaunch"), /manual restart/u],
  ["manual restart command removed", replace(files[16], "protocol_package_restart", "protocol_package_relaunch"), /manual restart/u],
  ["disabled local restart guard removed", replace(files[13], "if !stored.enabled", "if false"), /disabled local/u],
  ["process failure persistence removed", replace(files[2], "EXTERNAL_PACKAGE_PROCESS_FAILED", "EXTERNAL_PACKAGE_TRANSPORT_ERROR"), /process preparation/u],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
  });
}
