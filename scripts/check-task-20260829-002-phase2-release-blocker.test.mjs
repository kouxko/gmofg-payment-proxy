import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  findPhase2ReleaseBlockers,
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

test("current Phase 2 source is intentionally not release ready", async () => {
  const blockers = await findPhase2ReleaseBlockers();
  assert.equal(blockers.length, 2);

  const result = spawnSync(
    process.execPath,
    [path.join(import.meta.dirname, "check-task-20260829-002-phase2-release-blocker.mjs")],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 1);
  assert.match(result.stderr, /NOT_RELEASE_READY/u);
  assert.match(result.stderr, /marker/u);
  assert.match(result.stderr, /reset contract/u);
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

test("Tauri composition explicitly opts debug into recreate and release into preserve", async () => {
  const source = await readFile(
    path.join(import.meta.dirname, "../src-tauri/src/lib.rs"),
    "utf8",
  );
  assert.equal(source.split(marker).length - 1, 1);
  assert.match(
    source,
    /#\[cfg\(debug_assertions\)\][\s\S]*with_database_startup_policy\(DatabaseStartupPolicy::RecreateCurrent\)/u,
  );
  assert.match(
    source,
    /#\[cfg\(not\(debug_assertions\)\)\][\s\S]*DatabaseStartupPolicy::Preserve/u,
  );
  assert.doesNotMatch(source, /remove_file|\.sqlite3-wal|\.sqlite3-shm/u);
});
