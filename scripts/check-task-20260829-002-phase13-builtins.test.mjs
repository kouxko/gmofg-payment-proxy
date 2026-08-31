import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase13-builtins.mjs");
const files = [
  "src-tauri/Cargo.toml",
  "src-tauri/crates/infrastructure/Cargo.toml",
  "src-tauri/build.rs",
  "src-tauri/src/mcp/resources.rs",
  "test-support/fixtures/task-20260829-002/phase-7/package-runtime/inventory.json",
  "templates/socket-protocol/iso8583-standard/manifest.json",
  "templates/socket-protocol/iso8583-standard/protocol.js",
  "templates/socket-protocol/iso8583-standard/display.js",
  "test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json",
  "scripts/runtime-crate-dependencies.mjs",
  "scripts/check-rust-coverage.mjs",
  "src-tauri/crates/application/src/models/protocol_package.rs",
  "src-tauri/crates/application/src/ports/protocol_packages.rs",
  "src-tauri/crates/application/src/facade.rs",
  "src-tauri/crates/application/src/facade/protocol_packages/lookup.rs",
  "src-tauri/crates/infrastructure/src/adapters/bundle.rs",
  "src-tauri/crates/package-runtime/tests/phase13_builtin_package.rs",
];

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase13-builtins-"));
  for (const file of files) {
    const source = path.join(root, file);
    if (!fs.existsSync(source)) continue;
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(source, destination);
  }
  return target;
}

function run(cwd, env = {}) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    env: cwd === root
      ? { ...process.env, ...env }
      : { ...process.env, PHASE13_CHECKER_TEST_MODE: "sandbox", PHASE13_DISCOVERY_JSON: JSON.stringify([
          "strict_builtin_archive_executes_frame_decode_display_and_encode",
          "strict_builtin_archive_has_only_manifest_protocol_and_display",
        ]), PHASE13_PRODUCTION_DISCOVERY_JSON: JSON.stringify([
          "adapters::environment_apply_revision16_integration::internal_package_baseline::phase13_seed_projects_the_enabled_builtin_before_sidecar_start",
          "mcp::tests::g036_behavior_contract::application_lifecycle::production_apply::production_full_resource_candidate_requires_builtin_sidecar_online",
        ]), ...env },
    encoding: "utf8",
  });
}

test("canonical repository passes", () => assert.equal(run(root).status, 0));
for (const [name, mutate] of [
  ["legacy allowlist", (target) => {
    const file = path.join(target, files[4]);
    const value = JSON.parse(fs.readFileSync(file, "utf8"));
    value.legacy_internal_allowlist = [{ file: "legacy.rs", symbol: "parse", reason: "restored", owning_phase: "Phase13" }];
    fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
  }],
  ["Rhai dependency", (target) => fs.appendFileSync(path.join(target, files[0]), "\nrhai = \"1\"\n")],
  ["legacy template", (target) => fs.writeFileSync(path.join(target, "templates/socket-protocol/iso8583-standard/protocol.rhai"), "fn decode() {}")],
  ["missing JavaScript template", (target) => fs.unlinkSync(path.join(target, files[6]))],
  ["MCP legacy resource", (target) => fs.appendFileSync(path.join(target, files[3]), "\nconst OLD: &str = \"manifest.toml\";\n")],
  ["active legacy owner", (target) => fs.appendFileSync(path.join(target, files[9]), "\n// protocol-scripting\n")],
  ["Internal package source", (target) => fs.appendFileSync(path.join(target, files[11]), "\n// Internal {\n")],
  ["legacy package store port", (target) => fs.appendFileSync(path.join(target, files[12]), "\ntrait ProtocolPackageStorePort {}\n")],
  ["legacy package compiler port", (target) => fs.appendFileSync(path.join(target, files[12]), "\ntrait ProtocolPackageCompilerPort {}\n")],
  ["legacy package portability port", (target) => fs.appendFileSync(path.join(target, files[12]), "\ntrait ProtocolPackagePortabilityPort {}\n")],
  ["legacy Application store field", (target) => fs.appendFileSync(path.join(target, files[13]), "\n// protocol_package_store\n")],
  ["legacy lookup merge", (target) => fs.appendFileSync(path.join(target, files[14]), "\n// protocol_package_store\n")],
  ["legacy repository bundle wiring", (target) => fs.appendFileSync(path.join(target, files[15]), "\n// ProtocolPackageRepositoryAdapter\n")],
  ["legacy repository adapter stub", (target) => fs.writeFileSync(path.join(target, "src-tauri/crates/infrastructure/src/adapters/protocol_packages.rs"), "pub struct ProtocolPackageRepositoryAdapter;\n")],
  ["missing Display execution", (target) => {
    const file = path.join(target, files[16]);
    fs.writeFileSync(file, fs.readFileSync(file, "utf8").replace(".upstream_display(", ".upstream_decode("));
  }],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    assert.notEqual(run(target).status, 0);
  });
}

test("fails closed for Cargo zero-test discovery", () => {
  const target = sandbox();
  assert.notEqual(run(target, { PHASE13_DISCOVERY_JSON: "[]" }).status, 0);
});

test("fails closed for production Cargo zero-test discovery", () => {
  const target = sandbox();
  assert.notEqual(run(target, { PHASE13_PRODUCTION_DISCOVERY_JSON: "[]" }).status, 0);
});
