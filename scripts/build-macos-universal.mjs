import process from "node:process";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";

if (process.platform !== "darwin") {
  console.error("macOS Universal packaging requires macOS");
  process.exit(2);
}

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run("rustup", ["target", "add", "wasm32-wasip2"]);
run("deno", [
  "task",
  "tauri",
  "build",
  "--target",
  "universal-apple-darwin",
  "--bundles",
  "app",
  "--features",
  "macos-universal-vendored-openssl",
]);
run("deno", ["run", "-A", "scripts/sign-macos-app.mjs"]);
const app = path.resolve(
  "src-tauri/target/universal-apple-darwin/release/bundle/macos/Intercept Proxy.app",
);
const dmgDirectory = path.resolve(
  "src-tauri/target/universal-apple-darwin/release/bundle/dmg",
);
const dmg = path.join(dmgDirectory, "Intercept Proxy_1.0.0_universal.dmg");
const dmgSource = mkdtempSync(path.join(os.tmpdir(), "intercept-proxy-dmg-"));
try {
  cpSync(app, path.join(dmgSource, "Intercept Proxy.app"), { recursive: true });
  mkdirSync(dmgDirectory, { recursive: true });
  rmSync(dmg, { force: true });
  run("diskutil", [
    "image",
    "create",
    "from",
    "--format",
    "UDZO",
    "--volumeName",
    "Intercept Proxy",
    dmgSource,
    dmg,
  ]);
} finally {
  rmSync(dmgSource, { recursive: true, force: true });
}
run("deno", ["run", "-A", "scripts/verify-macos-universal.mjs"]);
