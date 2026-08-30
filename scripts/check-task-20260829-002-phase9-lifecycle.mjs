import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";

const paths = {
  binary: "src-tauri/crates/package-runtime/src/bin/intercept-proxy-package-sidecar.rs",
  processTest: "src-tauri/crates/package-runtime/tests/phase9_sidecar_process.rs",
  supervisor: "src-tauri/crates/infrastructure/src/adapters/local_package_supervisor.rs",
  supervisorTest: "src-tauri/crates/infrastructure/src/adapters/local_package_supervisor/tests.rs",
  importer: "src-tauri/crates/infrastructure/src/adapters/protocol_package_import.rs",
  importTest: "src-tauri/crates/infrastructure/src/adapters/protocol_package_import/tests.rs",
  registry: "src-tauri/crates/infrastructure/src/adapters/external_package_registry/mod.rs",
  localArchives: "src-tauri/crates/infrastructure/src/adapters/external_package_registry/local_archives.rs",
  registryPort: "src-tauri/crates/infrastructure/src/adapters/external_package_registry/application_port.rs",
  server: "src-tauri/crates/infrastructure/src/adapters/external_package_server.rs",
  bundle: "src-tauri/crates/infrastructure/src/adapters/bundle.rs",
  schema: "src-tauri/crates/infrastructure/src/sqlite/schema.rs",
  storage: "src-tauri/crates/infrastructure/src/sqlite/external_packages.rs",
  lifecycle: "src-tauri/crates/application/src/facade/protocol_packages.rs",
  restartLifecycle: "src-tauri/crates/application/src/facade/protocol_packages/lifecycle.rs",
  applicationPort: "src-tauri/crates/application/src/ports/external_packages.rs",
  commands: "src-tauri/src/commands/protocol_packages.rs",
  generated: "src/generated/rust-types.ts",
};
const source = Object.fromEntries(await Promise.all(Object.entries(paths).map(async ([key, file]) => [key, await readFile(file, "utf8")] )));
const failures = [];
const requireText = (key, text, message) => { if (!source[key].includes(text)) failures.push(message); };
const forbid = (key, pattern, message) => { if (pattern.test(source[key])) failures.push(message); };

for (const flag of ["--archive", "--packages-url"]) requireText("binary", flag, `Sidecar launch spec missing ${flag}`);
for (const text of ["connect_async", "PackageRegisterNotification::new", "PackageRpcRequest", "LocalSidecarRuntime::load"]) requireText("binary", text, `Sidecar process bridge missing ${text}`);
requireText("processTest", "process_initiates_registration_and_serves_fixed_rpc_until_killed", "real Sidecar process test missing");

for (const text of ["Duration::from_secs(10)", "kill_and_wait", "self.registry.disconnect", "wait_until_online", "start_enabled", "shutdown", "lifecycle_gate"]) requireText("supervisor", text, `supervisor lifecycle missing ${text}`);
forbid("supervisor", /retry|replay|backoff|Busy|rpc_timeout|max_in_flight/u, "supervisor must not add retry, replay, Busy, Hook timeout or queue policy");
requireText("supervisorTest", "restart_kills_and_reaps_old_process_before_next_launch", "restart ownership test missing");
requireText("supervisorTest", "shutdown_kills_every_owned_process_without_orphans", "shutdown orphan test missing");

for (const text of ["read_package_zip", "install_local_archive", "supervisor.launch", "pending"]) requireText("importer", text, `strict ZIP import path missing ${text}`);
forbid("importer", /LocalSidecarRuntime|\.load\s*\(/u, "Importer must not own or evaluate the Boa runtime");
requireText("schema", "local_archive BLOB NULL", "local ZIP must persist with exact package metadata");
requireText("storage", "install_local_external_package", "local ZIP atomic install missing");
requireText("storage", "enabled, first_connected_at", "local install must persist enabled state");
requireText("localArchives", "enabled_local_archives", "app-start exact enabled package query missing");
requireText("bundle", "supervisor.start_enabled", "app-start background Sidecar launch missing");
requireText("bundle", "with_local_supervisor", "Host shutdown ownership missing");
requireText("server", "supervisor.shutdown().await", "server shutdown must reap every local Sidecar");
requireText("registry", "contains_key(&package)", "duplicate exact online identity must fail without takeover");
requireText("registryPort", "supervisor.stop(package).await", "disable/delete must stop exact local Sidecar");
requireText("applicationPort", "async fn restart", "Application port manual restart missing");
requireText("restartLifecycle", "protocol_package_restart", "Application manual restart use case missing");
requireText("restartLifecycle", "if !stored.enabled", "disabled local package restart guard missing");
requireText("commands", "protocol_package_restart", "Tauri manual restart command missing");
requireText("generated", "protocolPackageRestart", "generated TypeScript manual restart binding missing");
requireText("supervisor", "EXTERNAL_PACKAGE_PROCESS_FAILED", "supervisor must persist process preparation failures");
requireText("lifecycle", "PROTOCOL_PACKAGE_DISABLED", "Listener start must gate disabled packages");
requireText("lifecycle", "EXTERNAL_PACKAGE_OFFLINE", "Listener start must gate offline packages");

const list = (packageName, filter, target) => {
  const args = ["test", "--manifest-path", "src-tauri/Cargo.toml", "-p", packageName];
  if (target) args.push("--test", target);
  args.push("--", "--list", "--format", "terse");
  const result = spawnSync("cargo", args, { encoding: "utf8" });
  if (result.status !== 0) return [];
  return result.stdout.split("\n").filter((line) => line.endsWith(": test") && line.includes(filter));
};
if (process.env.PHASE9_CHECKER_TEST_MODE !== "sandbox") {
  if (list("intercept-proxy-package-runtime", "", "phase9_sidecar_process").length < 2) failures.push("Cargo discovery lost Phase9 process tests");
  if (list("intercept-proxy-infrastructure", "local_package_supervisor").length < 2) failures.push("Cargo discovery lost supervisor tests");
}

if (failures.length) {
  for (const failure of failures) console.error(`FAIL: ${failure}`);
  process.exit(1);
}
console.log("PASS: Phase 9 local package lifecycle contract");
