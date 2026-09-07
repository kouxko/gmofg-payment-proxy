import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const generatedBindingsPath = path.join(repositoryRoot, "src/generated/rust-types.ts");

function runCargoBindingGenerator() {
  return new Promise((resolve, reject) => {
    const child = spawn(
      "cargo",
      [
        "run",
        "--release",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--features",
        "export-bindings",
        "--bin",
        "export-bindings",
      ],
      { cwd: repositoryRoot, stdio: "inherit" },
    );
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`binding generator failed with code ${code ?? "null"} signal ${signal ?? "none"}`));
    });
  });
}

export async function checkGeneratedBindings({
  generatedPath = generatedBindingsPath,
  runGenerator = runCargoBindingGenerator,
  read = readFile,
  write = writeFile,
} = {}) {
  const original = await read(generatedPath);
  let first;
  let second;
  try {
    await runGenerator();
    first = await read(generatedPath);
    await runGenerator();
    second = await read(generatedPath);
  } finally {
    await write(generatedPath, original);
  }

  const failures = [];
  if (!original.equals(first)) failures.push("src/generated/rust-types.ts is stale; run deno task bindings");
  if (!first.equals(second)) failures.push("binding generation is not deterministic across consecutive runs");
  return failures;
}

async function main() {
  const failures = await checkGeneratedBindings();
  if (failures.length > 0) {
    for (const failure of failures) process.stderr.write(`FAIL ${failure}\n`);
    process.exitCode = 1;
    return;
  }
  process.stdout.write("PASS generated bindings are fresh and deterministic\n");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
