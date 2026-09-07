import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

const root = process.argv[2]
  ? resolve(process.argv[2])
  : resolve(import.meta.dirname, "..");
const files = [
  "src-tauri/crates/infrastructure/src/sqlite/schema.rs",
  "src-tauri/crates/infrastructure/src/sqlite/external_packages.rs",
  "src-tauri/crates/infrastructure/src/sqlite/environment_configuration_baseline.rs",
  "src-tauri/crates/infrastructure/src/sqlite/workspaces.rs",
];

const failures = [];
for (const relative of [
  "scripts/e2e_proxy_rules.py",
  "scripts/test_e2e_proxy_rules.py",
]) {
  if (existsSync(resolve(root, relative))) {
    failures.push(`${relative}: obsolete Schema19 validation path remains`);
  }
}
for (const relative of files) {
  const source = readFileSync(resolve(root, relative), "utf8");
  if (/\bprotocol_packages\b|\bprotocol_package_files\b/u.test(source)) {
    failures.push(`${relative}: legacy package table/query remains`);
  }
}

const sqliteRoot = resolve(
  root,
  "src-tauri/crates/infrastructure/src/sqlite",
);
const pending = [sqliteRoot];
while (pending.length > 0) {
  const directory = pending.pop();
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) {
      pending.push(path);
      continue;
    }
    if (!entry.name.endsWith(".rs") || entry.name === "phase14_schema100.rs") {
      continue;
    }
    const source = readFileSync(path, "utf8");
    if (/\bprotocol_packages\b|\bprotocol_package_files\b/u.test(source)) {
      failures.push(`${path.slice(root.length + 1)}: legacy package table test remains`);
    }
  }
}

const schema = readFileSync(
  resolve(root, "src-tauri/crates/infrastructure/src/sqlite/schema.rs"),
  "utf8",
);
for (const required of [
  "CREATE TABLE workspaces",
  "CREATE TABLE external_protocol_packages",
  "registration_json TEXT NOT NULL",
  "local_archive BLOB NULL",
  "enabled INTEGER NOT NULL",
]) {
  if (!schema.includes(required)) {
    failures.push(`schema missing ${required}`);
  }
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("phase14 Schema100 contract: PASS");
