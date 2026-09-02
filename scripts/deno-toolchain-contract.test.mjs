import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const repositoryRoot = path.join(import.meta.dirname, "..");

async function readJson(relativePath) {
  return JSON.parse(
    await readFile(path.join(repositoryRoot, relativePath), "utf8"),
  );
}

async function readText(relativePath) {
  return await readFile(path.join(repositoryRoot, relativePath), "utf8");
}

test("Deno owns the frontend dependency and task contract", async () => {
  const packageJson = await readJson("package.json");
  const denoJson = await readJson("deno.json");
  const scripts = Object.values(packageJson.scripts ?? {});

  assert.equal(packageJson.packageManager, undefined);
  assert.equal(existsSync(path.join(repositoryRoot, "pnpm-lock.yaml")), false);
  assert.equal(existsSync(path.join(repositoryRoot, "deno.lock")), true);
  assert.equal(denoJson.nodeModulesDir, "auto");
  assert.deepEqual(denoJson.allowScripts, ["npm:unrs-resolver"]);
  assert.equal(
    scripts.some((command) =>
      /(?:^|&&|\|\||;)\s*(?:node|npm|pnpm)(?:\s|$)/u.test(command)
    ),
    false,
  );
});

test("Tauri development and formal builds invoke Deno", async () => {
  const baseConfig = await readJson("src-tauri/tauri.conf.json");
  const developmentConfig = await readJson("src-tauri/tauri.dev.conf.json");

  assert.equal(baseConfig.build.beforeDevCommand, "deno task dev");
  assert.equal(baseConfig.build.beforeBuildCommand, "deno task build");
  assert.equal(developmentConfig.build, undefined);
  assert.equal(
    existsSync(path.join(repositoryRoot, "src-tauri/tauri.deno.conf.json")),
    false,
  );
});

test("frontend CI and release workflows do not install or invoke Node and pnpm", async () => {
  const workflowPaths = [
    ".github/workflows/ci.yml",
    ".github/workflows/windows-release.yml",
    ".github/workflows/windows-quick-build.yml",
  ];

  for (const workflowPath of workflowPaths) {
    const source = await readText(workflowPath);
    assert.doesNotMatch(source, /actions\/setup-node|pnpm\/action-setup/u);
    assert.doesNotMatch(source, /(?:^|\s)(?:node|npm|pnpm)\s/u);
  }

  for (const workflowPath of workflowPaths.slice(0, 2)) {
    const source = await readText(workflowPath);
    assert.match(source, /denoland\/setup-deno@/u);
    assert.match(source, /deno ci/u);
  }
});

test("local and CI Rust toolchains are pinned to 1.98.0", async () => {
  const rustToolchain = await readText("rust-toolchain.toml");
  const cargoManifestPaths = [
    "src-tauri/Cargo.toml",
    "test-support/emulator-proxy-gate/Cargo.toml",
    "test-support/socket-relay-gate/Cargo.toml",
    "test-support/windows-platform-gate/Cargo.toml",
  ];
  const workflowPaths = [
    ".github/workflows/ci.yml",
    ".github/workflows/windows-release.yml",
    ".github/workflows/windows-quick-build.yml",
  ];

  assert.match(rustToolchain, /^channel = "1\.98\.0"$/mu);
  for (const cargoManifestPath of cargoManifestPaths) {
    const cargoManifest = await readText(cargoManifestPath);
    assert.match(
      cargoManifest,
      /^rust-version = "1\.98"$/mu,
      `${cargoManifestPath}: Rust version must be 1.98`,
    );
  }

  for (const workflowPath of workflowPaths) {
    const source = await readText(workflowPath);
    const rustToolchainActions = source.match(/dtolnay\/rust-toolchain@/gu) ?? [];
    const pinnedToolchains = source.match(/toolchain: 1\.98\.0/gu) ?? [];

    assert.ok(rustToolchainActions.length > 0, `${workflowPath}: Rust action missing`);
    assert.equal(
      pinnedToolchains.length,
      rustToolchainActions.length,
      `${workflowPath}: every Rust action must pin 1.98.0`,
    );
    assert.doesNotMatch(source, /toolchain:\s+1\.97(?:\.1)?/u);
  }
});
