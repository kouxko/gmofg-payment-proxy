import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
export const phase2ProductionSourcePaths = [
  "src-tauri/src/lib.rs",
  "src-tauri/crates/host/src/lib.rs",
  "src-tauri/crates/infrastructure/src/lib.rs",
  "src-tauri/crates/infrastructure/src/sqlite.rs",
  "src-tauri/crates/infrastructure/src/sqlite/core.rs",
  "src-tauri/crates/infrastructure/src/sqlite/executor.rs",
].map((sourcePath) => path.join(repositoryRoot, sourcePath));
export const phase17CurrentDocPaths = [
  "docs/architecture/rules-and-protocol-packages.md",
  "docs/architecture/modules.md",
  "docs/architecture/security-and-persistence.md",
  "docs/architecture/decisions/ADR-009-nested-document-javascript-package-runtime.md",
].map((sourcePath) => path.join(repositoryRoot, sourcePath));
const releaseBlockerMarker = [
  "TASK_20260829_002_PRE_RELEASE_",
  "DATABASE_RESET",
].join("");
const temporaryResetContract = new RegExp(
  [
    "\\bSqliteStartupPolicy\\b",
    "\\bDatabaseStartupPolicy\\b",
    "\\bRecreateCurrent\\b",
    "\\bwith_database_startup_policy\\b",
    "\\bopen_with_startup_policy\\b",
  ].join("|"),
  "gu",
);

const sqliteCorePath = path.join(
  repositoryRoot,
  "src-tauri/crates/infrastructure/src/sqlite/core.rs",
);

export async function findPhase2ReleaseBlockers({
  sourcePaths = phase2ProductionSourcePaths,
  read = readFile,
} = {}) {
  let markerCount = 0;
  let contractReferenceCount = 0;
  const markerPaths = new Set();
  const contractPaths = new Set();

  for (const sourcePath of sourcePaths) {
    const source = await read(sourcePath, "utf8");
    const sourceMarkerCount = source.split(releaseBlockerMarker).length - 1;
    const sourceContractCount = [...source.matchAll(temporaryResetContract)].length;
    markerCount += sourceMarkerCount;
    contractReferenceCount += sourceContractCount;
    if (sourceMarkerCount > 0) markerPaths.add(path.relative(repositoryRoot, sourcePath));
    if (sourceContractCount > 0) contractPaths.add(path.relative(repositoryRoot, sourcePath));
  }

  const blockers = [];
  if (markerCount > 0) {
    blockers.push(
      `${markerCount} temporary Phase 2 database reset marker(s) remain in ${[...markerPaths].join(", ")}`,
    );
  }
  if (contractReferenceCount > 0) {
    blockers.push(
      `${contractReferenceCount} temporary database startup reset contract reference(s) remain in ${[...contractPaths].join(", ")}`,
    );
  }
  return blockers;
}

const schema100StartupRequirements = [
  ["cross-process startup ownership", "acquire_startup_ownership(path)?"],
  ["exclusive startup ownership lock", "BEGIN EXCLUSIVE;"],
  ["pre-baseline schema revalidation", "match inspect_existing_schema(&connection)?", 2],
  ["pre-baseline version classification", "*version < CURRENT_SCHEMA_VERSION"],
  ["pre-baseline database cleanup call", "clear_pre_baseline_database(path)?"],
  ["SQLite WAL cleanup", 'sqlite_sidecar_path(path, "-wal")'],
  ["SQLite SHM cleanup", 'sqlite_sidecar_path(path, "-shm")'],
  ["SQLite main database cleanup", "std::fs::remove_file(path)"],
  ["invalid/future marker rejection", "InfrastructureError::DatabaseSchemaInvalid"],
];

export async function findSchema100StartupContractBlockers({
  coreSource,
  read = readFile,
} = {}) {
  const source = coreSource ?? (await read(sqliteCorePath, "utf8"));
  const missing = schema100StartupRequirements
    .filter(([, token, minimumCount = 1]) => source.split(token).length - 1 < minimumCount)
    .map(([label]) => label);
  return missing.length === 0
    ? []
    : [`Schema100 startup contract is missing: ${missing.join(", ")}`];
}

const currentSchema100Documentation = [
  /Schema 100[^。\n]*(?:原样保留|preserv)/iu,
  /(?:唯一有效版本标记[^。\n]*)?`?<\s*100`?[^。\n]*(?:清除|删除)[^。\n]*(?:重建|创建|recreat)[^。\n]*Schema 100/iu,
  /(?:未来版本|future)[^。\n]*(?:缺失|missing)[^。\n]*(?:重复|duplicate)[^。\n]*(?:损坏|malformed|corrupt)[^。\n]*fail-closed/iu,
];
const releaseFixtureName =
  "tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts";
const infrastructureCleanupFixtureName =
  "sqlite::core::tests::pre_schema100_startup_clears_legacy_data_and_recreates_schema100";
const hostCleanupFixtureName = "tests::pre_1_0_schema_is_cleared_and_recreated_as_schema100";

function discoverCargoTests(args) {
  const result = spawnSync("cargo", args, { cwd: repositoryRoot, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr || `cargo exited with status ${result.status}`);
  }
  return result.stdout
    .split("\n")
    .filter((line) => line.endsWith(": test"))
    .map((line) => line.slice(0, -6));
}

function discoverPhase17Fixtures() {
  return {
    infrastructure: discoverCargoTests([
      "test", "--manifest-path", "src-tauri/Cargo.toml", "-p",
      "intercept-proxy-infrastructure", "--lib", "--", "--list", "--format", "terse",
    ]),
    host: discoverCargoTests([
      "test", "--manifest-path", "src-tauri/Cargo.toml", "-p",
      "intercept-proxy-host", "--lib", "--", "--list", "--format", "terse",
    ]),
    release: discoverCargoTests([
      "test", "--release", "--manifest-path", "src-tauri/Cargo.toml", "-p",
      "intercept-proxy-host", "--lib", "--", "--list", "--format", "terse",
    ]),
  };
}

export async function findPhase17ReleaseContractBlockers({
  currentDocs,
  phase17Command,
  discoveredFixtures,
  read = readFile,
} = {}) {
  const documents =
    currentDocs ?? (await Promise.all(phase17CurrentDocPaths.map((docPath) => read(docPath, "utf8"))));
  const command =
    phase17Command ??
    JSON.parse(await read(path.join(repositoryRoot, "package.json"), "utf8")).scripts[
      "test:task-20260829-002:phase17"
    ];
  const blockers = [];

  if (
    documents.some((document) => {
      const normalized = document.replace(/\s+/gu, " ");
      return currentSchema100Documentation.some((requirement) => !requirement.test(normalized));
    })
  ) {
    blockers.push("current architecture documentation is missing the complete Schema100 startup contract");
  }
  if (documents.some((document) => /WAL\/SHM bytes/iu.test(document))) {
    blockers.push("current architecture documentation makes an unproven SHM byte-preservation claim");
  }
  const requiredExactCommands = [
    [
      `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib ${infrastructureCleanupFixtureName} -- --exact`,
      "Phase 17 aggregate must run the current pre-Schema100 cleanup exact fixture",
    ],
    [
      `cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib ${hostCleanupFixtureName} -- --exact`,
      "Phase 17 aggregate must run the current Host pre-1.0 cleanup exact fixture",
    ],
    [
      `cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib ${releaseFixtureName} -- --exact`,
      "Phase 17 aggregate must run the actual cargo test --release exact fixture",
    ],
  ];
  for (const [exactCommand, blocker] of requiredExactCommands) {
    if (!command?.includes(exactCommand)) blockers.push(blocker);
  }
  let discovered = discoveredFixtures;
  if (!discovered) {
    try {
      discovered = discoverPhase17Fixtures();
    } catch (error) {
      blockers.push(`Phase 17 Cargo discovery failed: ${error.message}`);
      return blockers;
    }
  }
  for (const [kind, fixtureName, label] of [
    ["infrastructure", infrastructureCleanupFixtureName, "Infrastructure cleanup"],
    ["host", hostCleanupFixtureName, "Host cleanup"],
    ["release", releaseFixtureName, "Release preserve"],
  ]) {
    const count = (discovered[kind] ?? []).filter((name) => name === fixtureName).length;
    if (count !== 1) {
      blockers.push(`Phase 17 Cargo discovery expected exactly one ${label} fixture, found ${count}`);
    }
  }
  return blockers;
}

async function main() {
  const blockers = [
    ...(await findPhase2ReleaseBlockers()),
    ...(await findSchema100StartupContractBlockers()),
    ...(await findPhase17ReleaseContractBlockers()),
  ];
  if (blockers.length === 0) {
    process.stdout.write("PASS Schema100 startup contract is release ready\n");
    return;
  }
  for (const blocker of blockers) process.stderr.write(`NOT_RELEASE_READY ${blocker}\n`);
  process.exitCode = 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
