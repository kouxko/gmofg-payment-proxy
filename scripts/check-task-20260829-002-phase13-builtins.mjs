import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import process from "node:process";

const root = "templates/socket-protocol/iso8583-standard";
const required = ["manifest.json", "protocol.js", "display.js"];
const forbidden = ["manifest.toml", "document.toml", "protocol.rhai", "display.rhai"];
const failures = [];
const failIf = (condition, message) => { if (condition) failures.push(message); };

for (const file of required) failIf(!existsSync(`${root}/${file}`), `missing strict built-in file ${file}`);
for (const file of forbidden) failIf(existsSync(`${root}/${file}`), `legacy built-in file remains: ${file}`);
failIf(existsSync("src-tauri/crates/protocol-scripting"), "legacy protocol-scripting crate remains");

const workspace = await readFile("src-tauri/Cargo.toml", "utf8");
const infrastructure = await readFile("src-tauri/crates/infrastructure/Cargo.toml", "utf8");
failIf(/^rhai\s*=/mu.test(workspace), "workspace still owns the Rhai dependency");
failIf(/^toml\s*=/mu.test(workspace), "workspace still owns the legacy TOML parser dependency");
failIf(infrastructure.includes("intercept-proxy-protocol-scripting"), "infrastructure still depends on protocol-scripting");

const phase7 = JSON.parse(await readFile("test-support/fixtures/task-20260829-002/phase-7/package-runtime/inventory.json", "utf8"));
failIf((phase7.legacy_internal_allowlist ?? []).length !== 0, "Phase 7 legacy allowlist must be empty after Phase 13");

const currentInventory = await readFile("test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json", "utf8");
const dependencyPolicy = await readFile("scripts/runtime-crate-dependencies.mjs", "utf8");
const coveragePolicy = await readFile("scripts/check-rust-coverage.mjs", "utf8");
for (const source of [currentInventory, dependencyPolicy, coveragePolicy]) {
  failIf(source.includes("protocol-scripting"), "active inventory and static policies must not retain protocol-scripting");
}

const applicationModel = await readFile("src-tauri/crates/application/src/models/protocol_package.rs", "utf8");
const applicationPorts = await readFile("src-tauri/crates/application/src/ports/protocol_packages.rs", "utf8");
const applicationFacade = await readFile("src-tauri/crates/application/src/facade.rs", "utf8");
const applicationLookup = await readFile("src-tauri/crates/application/src/facade/protocol_packages/lookup.rs", "utf8");
const bundle = await readFile("src-tauri/crates/infrastructure/src/adapters/bundle.rs", "utf8");
for (const [source, fragment, message] of [
  [applicationModel, "Internal {", "ProtocolPackageSourceViewModel must not retain Internal"],
  [applicationPorts, "ProtocolPackageStorePort", "legacy protocol package store port remains"],
  [applicationPorts, "ProtocolPackageCompilerPort", "legacy protocol package compiler port remains"],
  [applicationPorts, "ProtocolPackagePortabilityPort", "legacy protocol package portability port remains"],
  [applicationFacade, "protocol_package_store", "Application still owns the legacy internal store"],
  [applicationFacade, "protocol_package_compiler", "Application still owns the legacy internal compiler"],
  [applicationFacade, "protocol_package_portability: Arc<", "Application still owns the legacy portability port"],
  [applicationLookup, "protocol_package_store", "package lookup still merges internal and external sources"],
  [bundle, "ProtocolPackageRepositoryAdapter", "production bundle still wires the removed internal repository stub"],
]) failIf(source.includes(fragment), message);
failIf(existsSync("src-tauri/crates/infrastructure/src/adapters/protocol_packages.rs"), "legacy internal repository stub file remains");

const builtinCargoTest = await readFile("src-tauri/crates/package-runtime/tests/phase13_builtin_package.rs", "utf8");
failIf(!builtinCargoTest.includes(".upstream_display("), "built-in export test must execute Display");

const build = await readFile("src-tauri/build.rs", "utf8");
for (const file of required) failIf(!build.includes(file), `built-in ZIP builder must select ${file}`);
for (const file of forbidden) failIf(build.includes(file), `built-in ZIP builder references legacy ${file}`);

const mcp = await readFile("src-tauri/src/mcp/resources.rs", "utf8");
for (const file of required) failIf(!mcp.includes(file), `MCP resources must expose ${file}`);
for (const file of forbidden) failIf(mcp.includes(file), `MCP resources expose legacy ${file}`);

const expectedTests = [
  "strict_builtin_archive_executes_frame_decode_display_and_encode",
  "strict_builtin_archive_has_only_manifest_protocol_and_display",
];
const discoveredTests = process.env.PHASE13_CHECKER_TEST_MODE === "sandbox"
  ? JSON.parse(process.env.PHASE13_DISCOVERY_JSON ?? "[]")
  : (() => {
      const result = spawnSync("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-package-runtime", "--test", "phase13_builtin_package", "--", "--list", "--format", "terse"], { encoding: "utf8" });
      if (result.status !== 0) return [];
      return result.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6)).sort();
    })();
failIf(JSON.stringify(discoveredTests) !== JSON.stringify(expectedTests), "Cargo must discover the exact Phase 13 built-in tests");

const expectedProductionTests = [
  "adapters::environment_apply_revision16_integration::internal_package_baseline::phase13_seed_projects_the_enabled_builtin_before_sidecar_start",
  "mcp::tests::g036_behavior_contract::application_lifecycle::production_apply::production_full_resource_candidate_requires_builtin_sidecar_online",
];
const discoveredProductionTests = process.env.PHASE13_CHECKER_TEST_MODE === "sandbox"
  ? JSON.parse(process.env.PHASE13_PRODUCTION_DISCOVERY_JSON ?? "[]")
  : [
      spawnSync("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure", "--lib", "--all-features", "--", "--list", "--format", "terse"], { encoding: "utf8" }),
      spawnSync("cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy", "--lib", "--all-features", "--", "--list", "--format", "terse"], { encoding: "utf8" }),
    ].flatMap((result) => result.status === 0
      ? result.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6))
      : []).filter((name) => expectedProductionTests.includes(name)).sort();
failIf(JSON.stringify(discoveredProductionTests) !== JSON.stringify(expectedProductionTests), "Cargo must discover both full-name Phase 13 production tests; zero-test exact targets are forbidden");

if (failures.length) {
  for (const failure of failures) console.error(`FAIL: ${failure}`);
  process.exit(1);
}
console.log("PASS: Phase 13 built-in ZIP and legacy runtime deletion contract");
