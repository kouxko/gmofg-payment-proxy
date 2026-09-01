import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const workflowPath = path.join(import.meta.dirname, "../.github/workflows/windows-release.yml");

function job(source, name, nextName) {
  const start = source.indexOf(`  ${name}:`);
  const end = source.indexOf(`\n  ${nextName}:`, start + 1);
  assert.notEqual(start, -1, `missing ${name} job`);
  assert.notEqual(end, -1, `missing ${nextName} job after ${name}`);
  return source.slice(start, end);
}

test("manual build-only windows runs one Windows executable job without Android or macOS", async () => {
  const source = await readFile(workflowPath, "utf8");
  const androidJob = job(source, "android-companion", "verify");
  const verifyJob = job(source, "verify", "build-windows-executable");
  const executableJob = job(source, "build-windows-executable", "build");
  const installerJob = job(source, "build", "build-macos");
  const macosJob = source.slice(source.indexOf("  build-macos:"));

  assert.match(
    androidJob,
    /if: >-\s+github\.event_name == 'push' \|\|\s+inputs\.run_mode != 'build-only' \|\|\s+inputs\.platform == 'all'/u,
  );
  assert.match(executableJob, /runs-on: windows-latest/u);
  assert.match(
    executableJob,
    /github\.event_name == 'workflow_dispatch' &&\s+inputs\.run_mode == 'build-only' &&\s+inputs\.platform == 'windows'/u,
  );
  assert.match(
    executableJob,
    /cargo build --manifest-path src-tauri\/Cargo\.toml --release\s+--features tauri\/custom-protocol --bin intercept-proxy/u,
  );
  assert.match(executableJob, /TAURI_CONFIG: '\{"bundle":\{"resources":\[\]\}\}'/u);
  assert.match(executableJob, /src-tauri\/target\/release\/intercept-proxy\.exe/u);
  assert.match(verifyJob, /if: github\.event_name == 'push' \|\| inputs\.run_mode != 'build-only'/u);
  assert.match(installerJob, /needs\.android-companion\.result == 'success'/u);
  assert.match(macosJob, /\(github\.event_name == 'push' \|\| inputs\.platform == 'all'\)/u);
  assert.doesNotMatch(executableJob, /android-companion|build-macos|needs:/u);
  assert.doesNotMatch(executableJob, /pnpm audit|cargo test|cargo clippy/u);
});
