import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const releaseWorkflowPath = path.join(
  import.meta.dirname,
  "../.github/workflows/windows-release.yml",
);
const quickWorkflowPath = path.join(
  import.meta.dirname,
  "../.github/workflows/windows-quick-build.yml",
);

test("Windows quick workflow builds only one unsigned executable artifact", async () => {
  const source = await readFile(quickWorkflowPath, "utf8");

  assert.match(source, /^name: Windows quick executable$/mu);
  assert.match(source, /^on:\n  workflow_dispatch:\s*$/mu);
  assert.match(source, /group: windows-quick-executable-\$\{\{ github\.ref \}\}/u);
  assert.match(source, /permissions:\s+contents: read/u);
  assert.match(source, /^  build-windows-executable:$/mu);
  assert.match(source, /runs-on: windows-latest/u);
  assert.match(
    source,
    /cargo build --manifest-path src-tauri\/Cargo\.toml --release\s+--features tauri\/custom-protocol --bin intercept-proxy/u,
  );
  assert.match(source, /TAURI_CONFIG: '\{"bundle":\{"resources":\[\]\}\}'/u);
  assert.match(source, /src-tauri\/target\/release\/intercept-proxy\.exe/u);
  assert.match(source, /name: Intercept-Proxy-unsigned-executable-x64/u);
  assert.doesNotMatch(
    source,
    /android-companion|build-macos|Verify before packaging|pnpm audit|cargo test|cargo clippy|pull_request:|push:/u,
  );
  assert.equal((source.match(/^  [a-z][a-z0-9-]+:\s*$/gmu) ?? []).length, 1);
});

test("full desktop release never exposes the quick-build bypass", async () => {
  const source = await readFile(releaseWorkflowPath, "utf8");

  assert.match(source, /permissions:\s+contents: read/u);
  assert.match(source, /group: desktop-release-\$\{\{ github\.ref \}\}/u);
  assert.doesNotMatch(source, /group: windows-quick-executable-/u);
  assert.match(source, /^  android-companion:$/mu);
  assert.match(source, /^  verify:$/mu);
  assert.match(source, /^    timeout-minutes: 150$/mu);
  assert.match(source, /^  build:$/mu);
  assert.match(source, /^  build-macos:$/mu);
  assert.doesNotMatch(source, /build-only|build-windows-executable|inputs\.run_mode/u);
  assert.match(
    source,
    /needs\.android-companion\.result == 'success' && needs\.verify\.result == 'success'/u,
  );
  assert.match(source, /needs\.verify\.result == 'success' &&\s+\(github\.event_name/u);
});
