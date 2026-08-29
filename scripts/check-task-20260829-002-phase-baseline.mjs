import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const inventoryPath = path.join(
  repositoryRoot,
  "test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json",
);

function safeRepositoryPath(root, relativePath) {
  if (
    typeof relativePath !== "string" ||
    relativePath.length === 0 ||
    path.isAbsolute(relativePath)
  ) {
    return null;
  }
  const resolved = path.resolve(root, relativePath);
  return resolved.startsWith(`${root}${path.sep}`) ? resolved : null;
}

function duplicateIds(items) {
  const seen = new Set();
  return items
    .map((item) => item?.id)
    .filter((id) => typeof id === "string" && (seen.has(id) || !seen.add(id)));
}

export async function validateTaskPhaseBaseline({
  root = repositoryRoot,
  inventory,
  packageJson,
}) {
  const failures = [];
  if (inventory?.schema_version !== 1) failures.push("inventory schema_version must be 1");
  if (inventory?.task_id !== "TASK-20260829-002") failures.push("inventory task_id drifted");
  if (inventory?.phase !== 1) failures.push("inventory phase must be 1");

  const harnesses = Array.isArray(inventory?.harnesses) ? inventory.harnesses : [];
  const baselines = Array.isArray(inventory?.baselines) ? inventory.baselines : [];
  if (harnesses.length === 0) failures.push("inventory must declare compileable harness locations");
  if (baselines.length === 0) failures.push("inventory must declare current-state baselines");
  for (const duplicate of duplicateIds([...harnesses, ...baselines])) {
    failures.push(`duplicate inventory id: ${duplicate}`);
  }

  const checkpoint = inventory?.checkpoint;
  const commands = Array.isArray(checkpoint?.commands) ? checkpoint.commands : [];
  const requiredCommands = [
    "pnpm test:task-20260829-002:phase1",
    "pnpm check:bindings",
    "pnpm scan:architecture",
    "pnpm scan:source-size",
    "pnpm lint",
    "pnpm typecheck",
    "pnpm test",
    "pnpm check:rust:fmt",
    "pnpm check:rust:clippy",
    "cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets --all-features",
  ];
  if (
    commands.length !== requiredCommands.length ||
    commands.some((command, index) => command !== requiredCommands[index])
  ) {
    failures.push("checkpoint commands drifted from the required command sequence");
  }
  const packageScript = packageJson?.scripts?.[checkpoint?.package_script];
  if (packageScript !== commands.join(" && ")) {
    failures.push(`package script ${checkpoint?.package_script ?? "<missing>"} drifted from checkpoint commands`);
  }

  for (const entry of [...harnesses, ...baselines]) {
    if (!Number.isInteger(entry?.owner_phase) || entry.owner_phase < 2 || entry.owner_phase > 17) {
      failures.push(`${entry?.id ?? "<missing-id>"}: owner_phase must be between 2 and 17`);
    }
    const resolved = safeRepositoryPath(root, entry?.path);
    if (resolved === null) {
      failures.push(`${entry?.id ?? "<missing-id>"}: path must stay inside the repository`);
      continue;
    }
    try {
      await access(resolved);
    } catch {
      failures.push(`${entry.id}: missing harness/source path ${entry.path}`);
      continue;
    }
    if (!Array.isArray(entry.required_fragments)) continue;
    const source = await readFile(resolved, "utf8");
    for (const fragment of entry.required_fragments) {
      if (!source.includes(fragment)) {
        failures.push(`${entry.id}: missing current-state fragment ${JSON.stringify(fragment)}`);
      }
    }
  }
  return failures;
}

async function main() {
  const [inventorySource, packageSource] = await Promise.all([
    readFile(inventoryPath, "utf8"),
    readFile(path.join(repositoryRoot, "package.json"), "utf8"),
  ]);
  const failures = await validateTaskPhaseBaseline({
    inventory: JSON.parse(inventorySource),
    packageJson: JSON.parse(packageSource),
  });
  if (failures.length > 0) {
    for (const failure of failures) process.stderr.write(`FAIL ${failure}\n`);
    process.exitCode = 1;
    return;
  }
  process.stdout.write(
    "PASS TASK-20260829-002 Phase 1 inventory, harness locations, generated-type baseline, and checkpoint commands\n",
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
