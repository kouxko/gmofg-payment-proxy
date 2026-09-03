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
  assert.match(
    source,
    /group: windows-quick-executable-\$\{\{ github\.ref \}\}/u,
  );
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
  assert.doesNotMatch(
    source,
    /build-only|build-windows-executable|inputs\.run_mode/u,
  );
  assert.match(
    source,
    /needs\.android-companion\.result == 'success' && needs\.verify\.result == 'success'/u,
  );
  assert.match(
    source,
    /needs\.verify\.result == 'success' &&\s+\(github\.event_name/u,
  );
});

test("full desktop release shares one cached Cargo target across runtime gates", async () => {
  const source = await readFile(releaseWorkflowPath, "utf8");
  const verifyJob = source.slice(
    source.indexOf("  verify:"),
    source.indexOf("  build:"),
  );

  assert.match(
    verifyJob,
    /name: Verify independent runtime gates\s+env:\s*\n(?:\s*#.*\n)*\s*CARGO_TARGET_DIR: \$\{\{ github\.workspace \}\}\/src-tauri\/target/u,
  );
  assert.match(verifyJob, /workspaces: src-tauri -> target/u);
  assert.match(
    verifyJob,
    /test-support\/emulator-proxy-gate\/Cargo\.toml/u,
  );
  assert.match(
    verifyJob,
    /test-support\/socket-relay-gate\/Cargo\.toml/u,
  );
});

test("tagged desktop releases distinguish complete signing from explicit unsigned mode", async () => {
  const source = await readFile(releaseWorkflowPath, "utf8");

  assert.match(source, /name: Resolve Windows signing mode/u);
  assert.match(source, /if \(\$configuredCount -eq 0\)/u);
  assert.match(source, /"mode=unsigned" >> \$env:GITHUB_OUTPUT/u);
  assert.match(source, /elseif \(\$configuredCount -eq 3\)/u);
  assert.match(source, /"mode=signed" >> \$env:GITHUB_OUTPUT/u);
  assert.match(
    source,
    /Windows signing configuration must be either complete or absent/u,
  );
  assert.match(
    source,
    /startsWith\(github\.ref, 'refs\/tags\/v'\) && steps\.windows_signing_mode\.outputs\.mode == 'signed'/u,
  );
  assert.match(source, /name: Verify unsigned Windows release artifacts/u);
  assert.match(source, /\$signature\.Status -ne "NotSigned"/u);
  assert.match(
    source,
    /steps\.windows_signing_mode\.outputs\.mode == 'signed' && 'Intercept-Proxy-signed-installers-x64' \|\| 'Intercept-Proxy-unsigned-installers-x64'/u,
  );
  assert.match(
    source,
    /steps\.windows_signing_mode\.outputs\.mode == 'signed' && 'Intercept-Proxy-signed-portable-x64' \|\| 'Intercept-Proxy-unsigned-portable-x64'/u,
  );
});
