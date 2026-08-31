import { chmod, copyFile, mkdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";

const target = process.argv[2];
if (!target) {
  console.error("usage: node scripts/stage-package-sidecar.mjs <rust-target-triple>");
  process.exit(2);
}

const windows = target.includes("windows");
const executable = `intercept-proxy-package-sidecar${windows ? ".exe" : ""}`;
const source = path.join("src-tauri", "target", target, "release", executable);
const destination = path.join(
  "src-tauri",
  "binaries",
  `intercept-proxy-package-sidecar-${target}${windows ? ".exe" : ""}`,
);

const build = spawnSync(
  "cargo",
  [
    "build",
    "--release",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "-p",
    "intercept-proxy-package-runtime",
    "--bin",
    "intercept-proxy-package-sidecar",
    "--target",
    target,
  ],
  { stdio: "inherit" },
);
if (build.status !== 0) process.exit(build.status ?? 1);

await mkdir(path.dirname(destination), { recursive: true });
await copyFile(source, destination);
if (!windows) await chmod(destination, 0o755);
console.log(destination);
