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

async function main() {
  const blockers = await findPhase2ReleaseBlockers();
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
