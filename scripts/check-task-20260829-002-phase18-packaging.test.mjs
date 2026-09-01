import assert from "node:assert/strict";
import fs from "node:fs";
import { readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const repositoryRoot = path.resolve(import.meta.dirname, "..");

test("macOS Universal release shape bundles the Boa sidecar and owns build validation", async () => {
  const tauriConfig = JSON.parse(
    await readFile(path.join(repositoryRoot, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const packageJson = JSON.parse(
    await readFile(path.join(repositoryRoot, "package.json"), "utf8"),
  );

  assert.deepEqual(tauriConfig.bundle.externalBin, [
    "binaries/intercept-proxy-package-sidecar",
  ]);
  assert.equal(
    packageJson.scripts["build:macos:universal"],
    "node scripts/build-macos-universal.mjs",
  );
  assert.equal(
    packageJson.scripts["verify:macos:universal"],
    "node scripts/verify-macos-universal.mjs",
  );
});

test("release process E2E consumes the current Schema100 database only", async () => {
  const source = await readFile(
    path.join(repositoryRoot, "scripts/e2e_external_packages.py"),
    "utf8",
  );
  assert.match(source, /EXPECTED_SCHEMA_VERSION = 100/u);
  assert.doesNotMatch(source, /Expected database schema 19|version != \(19,\)/u);
});

test("Windows installer build stages its own target sidecar before Tauri packaging", async () => {
  const workflow = await readFile(
    path.join(repositoryRoot, ".github/workflows/windows-release.yml"),
    "utf8",
  );
  const buildJob = workflow.slice(
    workflow.indexOf("  build:\n"),
    workflow.indexOf("  build-macos:\n"),
  );
  const stageIndex = buildJob.indexOf(
    "node scripts/stage-package-sidecar.mjs x86_64-pc-windows-msvc",
  );
  const packageIndex = buildJob.indexOf("- name: Build MSI and NSIS installers");

  assert.notEqual(stageIndex, -1, "Windows build job must stage the Boa sidecar");
  assert.notEqual(packageIndex, -1, "Windows build job must package MSI and NSIS");
  assert.ok(stageIndex < packageIndex, "sidecar staging must precede Tauri packaging");
});

const checker = path.join(repositoryRoot, "scripts/check-task-20260829-002-phase18-packaging.mjs");
const checkerFiles = [
  "src-tauri/tauri.conf.json",
  "package.json",
  "scripts/stage-package-sidecar.mjs",
  "scripts/build-macos-universal.mjs",
  "scripts/verify-macos-universal.mjs",
  "scripts/sign-macos-app.mjs",
  "scripts/e2e_macos_mounted_release.py",
  "scripts/e2e_external_packages.py",
  ".github/workflows/windows-release.yml",
  "src-tauri/src/lib.rs",
  "src-tauri/Cargo.toml",
  "src-tauri/crates/proxy/Cargo.toml",
];

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase18-packaging-"));
  for (const file of checkerFiles) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(repositoryRoot, file), destination);
  }
  return target;
}

function check(target = repositoryRoot) {
  return spawnSync(process.execPath, [checker], {
    cwd: repositoryRoot,
    encoding: "utf8",
    env: { ...process.env, PHASE18_CHECK_ROOT: target },
  });
}

test("Phase18 checker accepts the canonical repository", () => {
  const result = check();
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, file, before, after, expected] of [
  ["externalBin drift", checkerFiles[0], "binaries/intercept-proxy-package-sidecar", "binaries/wrong-sidecar", /externalBin/u],
  ["one macOS architecture dropped", checkerFiles[3], "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", /x86_64-apple-darwin/u],
  ["Universal sidecar merge dropped", checkerFiles[3], 'run("lipo", [\n  "-create",', 'run("true", [\n  "-create",', /Universal sidecar merge/u],
  ["signed App DMG creation dropped", checkerFiles[3], 'run("diskutil", [', 'run("true", [', /signed App DMG creation/u],
  ["deprecated hdiutil create restored", checkerFiles[3], 'run("diskutil", [\n    "image",\n    "create",\n    "from",', 'run("hdiutil", [\n    "create",', /deprecated hdiutil create/u],
  ["sidecar lipo verification dropped", checkerFiles[4], "for (const binary of [mainBinary, sidecar])", "for (const binary of [mainBinary])", /mainBinary, sidecar/u],
  ["bundle ad-hoc seal dropped", checkerFiles[5], '"--sign"', '"--display"', /--sign/u],
  ["mounted DMG dropped", checkerFiles[6], '"hdiutil", "attach"', '"cp", "app"', /hdiutil/u],
  ["isolated profile dropped", checkerFiles[6], 'environment["CFFIXED_USER_HOME"]', 'environment["SHARED_USER_HOME"]', /CFFIXED_USER_HOME/u],
  ["Socket official ZIP dropped", checkerFiles[6], "iso8583-ascii-standard", "direct-socket", /iso8583-ascii-standard/u],
  [
    "Windows sidecar target dropped",
    checkerFiles[8],
    "      - name: Stage Windows Boa package sidecar\n        run: node scripts/stage-package-sidecar.mjs x86_64-pc-windows-msvc\n\n      - name: Configure Authenticode signing for tagged releases",
    "      - name: Stage Windows Boa package sidecar\n        run: node scripts/stage-package-sidecar.mjs x86_64-apple-darwin\n\n      - name: Configure Authenticode signing for tagged releases",
    /Windows build job/u,
  ],
  [
    "Windows build sidecar staging dropped",
    checkerFiles[8],
    "      - name: Stage Windows Boa package sidecar\n        run: node scripts/stage-package-sidecar.mjs x86_64-pc-windows-msvc\n\n      - name: Configure Authenticode signing for tagged releases",
    "      - name: Configure Authenticode signing for tagged releases",
    /Windows build job/u,
  ],
  ["Schema19 restored", checkerFiles[7], "EXPECTED_SCHEMA_VERSION = 100", "EXPECTED_SCHEMA_VERSION = 19", /Schema100/u],
  ["Release imports debug-gated", checkerFiles[9], "use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};", "#[cfg(debug_assertions)]\nuse intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};", /debug-only/u],
  ["Universal vendored OpenSSL dropped", checkerFiles[10], 'macos-universal-vendored-openssl = ["intercept-proxy-runtime/macos-universal-vendored-openssl"]', 'macos-universal-vendored-openssl = []', /root Cargo/u],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    const sourceFile = path.join(target, file);
    fs.writeFileSync(sourceFile, fs.readFileSync(sourceFile, "utf8").replace(before, after));
    const result = check(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
  });
}
