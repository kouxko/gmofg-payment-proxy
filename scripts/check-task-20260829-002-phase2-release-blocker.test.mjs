import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  findPhase2ReleaseBlockers,
  findPhase17ReleaseContractBlockers,
  phase2ProductionSourcePaths,
} from "./check-task-20260829-002-phase2-release-blocker.mjs";

const marker = ["TASK_20260829_002_PRE_RELEASE_", "DATABASE_RESET"].join("");
const fixturePaths = [
  "/fixture/src-tauri/src/lib.rs",
  "/fixture/src-tauri/crates/host/src/lib.rs",
  "/fixture/src-tauri/crates/infrastructure/src/sqlite.rs",
];

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

test("release scan blocks source comments that reintroduce clearing pre-release databases", async () => {
  const blockers = await scanFixture(
    new Map([[fixturePaths[2], "// 启动时仅清空合法的发布前版本。\n"]]),
  );
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /temporary database startup reset contract/u);
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

test("current Phase 17 source is release ready after the temporary reset contract is removed", async () => {
  const blockers = await findPhase2ReleaseBlockers();
  assert.deepEqual(blockers, []);

  const result = spawnSync(
    process.execPath,
    [path.join(import.meta.dirname, "check-task-20260829-002-phase2-release-blocker.mjs")],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.match(result.stdout, /PASS no temporary Phase 2 database reset remains/u);
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

test("Tauri composition has one preserve-only Host startup path", async () => {
  const source = await readFile(
    path.join(import.meta.dirname, "../src-tauri/src/lib.rs"),
    "utf8",
  );
  assert.equal(source.split(marker).length - 1, 0);
  assert.doesNotMatch(source, /DatabaseStartupPolicy|with_database_startup_policy|RecreateCurrent/u);
  assert.doesNotMatch(source, /remove_file|\.sqlite3-wal|\.sqlite3-shm/u);
});

test("Phase 17 release scan blocks current docs that reintroduce a pre-100 reset path", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: ["当前 development recreate branch 可重建 Schema 100。"],
    phase17Command:
      "cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts -- --exact",
  });
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /current architecture documentation reintroduces pre-100 reset/u);
});

test("Phase 17 release scan blocks an aggregate command that drops the release profile", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: ["Schema 100 is preserved; pre-100 fails closed without mutation."],
    phase17Command:
      "cargo test --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts -- --exact",
  });
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /actual cargo test --release exact fixture/u);
});

test("Phase 17 release scan blocks unproven SHM byte preservation claims", async () => {
  const blockers = await findPhase17ReleaseContractBlockers({
    currentDocs: ["失败不得改写主数据库、WAL/SHM bytes 或用户数据。"],
    phase17Command:
      "cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts -- --exact",
  });
  assert.equal(blockers.length, 1);
  assert.match(blockers[0], /unproven SHM byte-preservation/u);
});
