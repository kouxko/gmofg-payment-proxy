import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  findPhase2ReleaseBlockers,
  findPhase17ReleaseContractBlockers,
  findSchema100StartupContractBlockers,
  phase2ProductionSourcePaths,
} from "./check-task-20260829-002-phase2-release-blocker.mjs";

const marker = ["TASK_20260829_002_PRE_RELEASE_", "DATABASE_RESET"].join("");
const fixturePaths = [
  "/fixture/src-tauri/src/lib.rs",
  "/fixture/src-tauri/crates/host/src/lib.rs",
  "/fixture/src-tauri/crates/infrastructure/src/sqlite.rs",
];
const infrastructureCleanupExact =
  "cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib sqlite::core::tests::pre_schema100_startup_clears_legacy_data_and_recreates_schema100 -- --exact";
const hostCleanupExact =
  "cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::pre_1_0_schema_is_cleared_and_recreated_as_schema100 -- --exact";
const releaseExact =
  "cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts -- --exact";
const currentPhase17Command = [infrastructureCleanupExact, hostCleanupExact, releaseExact].join(
  " && ",
);
const currentPhase17Discovery = {
  infrastructure: [
    "sqlite::core::tests::pre_schema100_startup_clears_legacy_data_and_recreates_schema100",
  ],
  host: ["tests::pre_1_0_schema_is_cleared_and_recreated_as_schema100"],
  release: [
    "tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts",
  ],
};

async function scanFixture(sources) {
  return findPhase2ReleaseBlockers({
    sourcePaths: fixturePaths,
    read: async (sourcePath) => sources.get(sourcePath) ?? "",
  });
}

test("release scan passes only after the complete temporary reset contract is removed", async () => {
  assert.deepEqual(await scanFixture(new Map()), []);
});

test("release scan blocks when marker is removed but the debug recreate opt-in remains", async () => {
  const blockers = await scanFixture(
    new Map([
      [
        fixturePaths[0],
        "host_builder.with_database_startup_policy(DatabaseStartupPolicy::RecreateCurrent);\n",
      ],
    ]),
  );
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /temporary database startup reset contract/u);
});

test("release scan blocks when opt-in is removed but the temporary policy remains", async () => {
  const blockers = await scanFixture(
    new Map([
      [fixturePaths[2], "pub enum SqliteStartupPolicy { Preserve, RecreateCurrent }\n"],
    ]),
  );
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /temporary database startup reset contract/u);
});

test("release scan blocks a marker even when the reset contract is absent", async () => {
  const blockers = await scanFixture(
    new Map([[fixturePaths[0], `const BLOCKER: &str = "${marker}";\n`]]),
  );
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /temporary Phase 2 database reset marker/u);
});

test("release scan reports each blocker category once and scans production Rust only", async () => {
  const reads = [];
  const blockers = await findPhase2ReleaseBlockers({
    sourcePaths: fixturePaths,
    read: async (sourcePath) => {
      reads.push(sourcePath);
      return `${marker}\nDatabaseStartupPolicy::RecreateCurrent\n`.repeat(2);
    },
  });
  assert.equal(blockers.length, 2);
  assert.deepEqual(reads, fixturePaths);
  assert.ok(phase2ProductionSourcePaths.every((sourcePath) => sourcePath.endsWith(".rs")));
  assert.ok(phase2ProductionSourcePaths.every((sourcePath) => !sourcePath.includes("/tests/")));
  assert.ok(phase2ProductionSourcePaths.every((sourcePath) => !sourcePath.includes("/docs/")));
});

test("Schema100 startup contract requires exact pre-baseline cleanup and invalid-marker rejection", async () => {
  const source = `
    let _startup_ownership = acquire_startup_ownership(path)?;
    "BEGIN EXCLUSIVE;";
    match inspect_existing_schema(&connection)? {}
    match inspect_existing_schema(&connection)? {}
    [(1, version)] if *version < CURRENT_SCHEMA_VERSION => ExistingSchema::PreBaseline(*version),
    clear_pre_baseline_database(path)?;
    sqlite_sidecar_path(path, "-wal");
    sqlite_sidecar_path(path, "-shm");
    std::fs::remove_file(path);
    InfrastructureError::DatabaseSchemaInvalid;
  `;
  assert.deepEqual(await findSchema100StartupContractBlockers({ coreSource: source }), []);

  for (const removed of [
    "acquire_startup_ownership(path)?",
    "BEGIN EXCLUSIVE;",
    "match inspect_existing_schema(&connection)?",
    "*version < CURRENT_SCHEMA_VERSION",
    "clear_pre_baseline_database(path)?",
    'sqlite_sidecar_path(path, "-wal")',
    'sqlite_sidecar_path(path, "-shm")',
    "std::fs::remove_file(path)",
    "InfrastructureError::DatabaseSchemaInvalid",
  ]) {
    const blockers = await findSchema100StartupContractBlockers({
      coreSource: source.replace(removed, ""),
    });
    assert.equal(blockers.length, 1, `removing ${removed} must fail closed`);
  }
});

test("current Schema100 startup source is release ready", async () => {
  const blockers = await findPhase2ReleaseBlockers();
  assert.deepEqual(blockers, []);
  assert.deepEqual(await findSchema100StartupContractBlockers(), []);

  const result = spawnSync(
    process.execPath,
    [path.join(import.meta.dirname, "check-task-20260829-002-phase2-release-blocker.mjs")],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.match(result.stdout, /PASS Schema100 startup contract is release ready/u);
});

test("tauri build cannot bypass the release readiness gate", async () => {
  const packageJson = JSON.parse(
    await readFile(path.join(import.meta.dirname, "../package.json"), "utf8"),
  );
  const tauriConfig = JSON.parse(
    await readFile(path.join(import.meta.dirname, "../src-tauri/tauri.conf.json"), "utf8"),
  );
  assert.equal(
    packageJson.scripts["tauri:build"],
    "pnpm check:task-20260829-002:phase2-release-ready && pnpm build:android-companion && tauri build",
  );
  assert.equal(packageJson.scripts.tauri, "tauri");
  assert.equal(
    tauriConfig.build.beforeBuildCommand,
    "pnpm check:task-20260829-002:phase2-release-ready && pnpm build",
  );
  assert.equal(tauriConfig.build.beforeDevCommand, "pnpm dev");
  assert.equal(
    packageJson.scripts["tauri:dev"],
    "tauri dev --config src-tauri/tauri.dev.conf.json",
  );
});

test("Tauri composition delegates the single Schema100 startup path to Host", async () => {
  const source = await readFile(
    path.join(import.meta.dirname, "../src-tauri/src/lib.rs"),
    "utf8",
  );
  assert.equal(source.split(marker).length - 1, 0);
  assert.doesNotMatch(source, /DatabaseStartupPolicy|with_database_startup_policy|RecreateCurrent/u);
  assert.doesNotMatch(source, /remove_file|\.sqlite3-wal|\.sqlite3-shm/u);
});

test("Phase 17 release scan requires the complete current Schema100 startup contract", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: ["Schema 100 is preserved; pre-100 fails closed without mutation."],
    phase17Command: currentPhase17Command,
    discoveredFixtures: currentPhase17Discovery,
  });
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /complete Schema100 startup contract/u);
});

test("Phase 17 release scan blocks an aggregate command that drops the release profile", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: [
      "Schema 100 原样保留；唯一有效版本标记 <100 时清除旧数据库并重建 Schema 100；未来版本、缺失、重复或损坏标记 fail-closed。",
    ],
    phase17Command: [
      infrastructureCleanupExact,
      hostCleanupExact,
      releaseExact.replace("cargo test --release", "cargo test"),
    ].join(" && "),
    discoveredFixtures: currentPhase17Discovery,
  });
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /actual cargo test --release exact fixture/u);
});

test("Phase 17 release scan rejects stale exact filters that execute zero tests", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: [
      "Schema 100 原样保留；唯一有效版本标记 <100 时清除旧数据库并重建 Schema 100；未来版本、缺失、重复或损坏标记 fail-closed。",
    ],
    phase17Command:
      "cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-infrastructure --lib sqlite::core::tests::preserve_only_startup_rejects_pre_schema100_without_modifying_it -- --exact && cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::pre_1_0_schema_is_rejected_without_changing_any_sqlite_file -- --exact && cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts -- --exact",
    discoveredFixtures: currentPhase17Discovery,
  });
  assert.equal(blockers.length, 2);
  assert.match(blockers[0], /pre-Schema100 cleanup exact fixture/u);
  assert.match(blockers[1], /Host pre-1\.0 cleanup exact fixture/u);
});

test("Phase 17 release scan rejects zero-test Cargo discovery", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: [
      "Schema 100 原样保留；唯一有效版本标记 <100 时清除旧数据库并重建 Schema 100；未来版本、缺失、重复或损坏标记 fail-closed。",
    ],
    phase17Command: currentPhase17Command,
    discoveredFixtures: { infrastructure: [], host: [], release: [] },
  });
  assert.equal(blockers.length, 3);
  assert.ok(blockers.every((blocker) => /Cargo discovery expected exactly one/u.test(blocker)));
});

test("Phase 17 release scan blocks unproven SHM byte preservation claims", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: [
      "Schema 100 原样保留；唯一有效版本标记 <100 时清除旧数据库并重建 Schema 100；未来版本、缺失、重复或损坏标记 fail-closed。失败不得改写主数据库、WAL/SHM bytes 或用户数据。",
    ],
    phase17Command: currentPhase17Command,
    discoveredFixtures: currentPhase17Discovery,
  });
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /unproven SHM byte-preservation/u);
});
