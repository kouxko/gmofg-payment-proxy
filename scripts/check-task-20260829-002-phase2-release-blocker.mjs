import { readFile } from "node:fs/promises";
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
    "启动时仅清空(?:合法的)?发布前版本",
  ].join("|"),
  "gu",
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

const currentResetDocumentation =
  /development recreate|当前开发启动[^。\n]*(?:重建|清理)|开发启动重建|Phase17 (?:删除前|必须删除)|Phase17[^。\n]*发布前删除/iu;
const releaseFixtureName =
  "tests::phase2_database_startup::release_startup_preserves_schema100_state_across_two_real_host_starts";

export async function findPhase17ReleaseContractBlockers({
  currentDocs,
  phase17Command,
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

  if (documents.some((document) => currentResetDocumentation.test(document))) {
    blockers.push("current architecture documentation reintroduces pre-100 reset semantics");
  }
  if (documents.some((document) => /WAL\/SHM bytes/iu.test(document))) {
    blockers.push("current architecture documentation makes an unproven SHM byte-preservation claim");
  }
  const releaseExact = `cargo test --release --manifest-path src-tauri/Cargo.toml -p intercept-proxy-host --lib ${releaseFixtureName} -- --exact`;
  if (!command?.includes(releaseExact)) {
    blockers.push("Phase 17 aggregate must run the actual cargo test --release exact fixture");
  }
  return blockers;
}

async function main() {
  const blockers = [
    ...(await findPhase2ReleaseBlockers()),
    ...(await findPhase17ReleaseContractBlockers()),
  ];
  if (blockers.length === 0) {
    process.stdout.write("PASS no temporary Phase 2 database reset remains\n");
    return;
  }
  for (const blocker of blockers) process.stderr.write(`NOT_RELEASE_READY ${blocker}\n`);
  process.exitCode = 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
