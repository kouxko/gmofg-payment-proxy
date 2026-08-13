import { spawnSync } from "node:child_process";
import path from "node:path";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const windowsTarget = "x86_64-pc-windows-msvc";
const gateManifest = path.join(
  repositoryRoot,
  "test-support/windows-platform-gate/Cargo.toml",
);

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repositoryRoot,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run("rustup", ["target", "add", windowsTarget]);
run("cargo", [
  "clippy",
  "--manifest-path",
  gateManifest,
  "--lib",
  "--target",
  windowsTarget,
  "--",
  "-D",
  "warnings",
]);
