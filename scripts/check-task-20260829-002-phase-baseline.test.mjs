import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { validateTaskPhaseBaseline } from "./check-task-20260829-002-phase-baseline.mjs";

const repositoryRoot = path.resolve(import.meta.dirname, "..");

async function baselineInputs() {
  const [inventorySource, packageSource] = await Promise.all([
    readFile(
      path.join(
        repositoryRoot,
        "test-support/fixtures/task-20260829-002/phase-1/current-contract-inventory.json",
      ),
      "utf8",
    ),
    readFile(path.join(repositoryRoot, "package.json"), "utf8"),
  ]);
  return {
    inventory: JSON.parse(inventorySource),
    packageJson: JSON.parse(packageSource),
  };
}

test("current TASK-20260829-002 contract inventory is executable and GREEN", async () => {
  const inputs = await baselineInputs();
  assert.deepEqual(
    await validateTaskPhaseBaseline({ root: repositoryRoot, ...inputs }),
    [],
  );
});

test("baseline validation fails closed when checkpoint commands drift", async () => {
  const inputs = await baselineInputs();
  inputs.packageJson.scripts[inputs.inventory.checkpoint.package_script] = "pnpm typecheck";
  assert.match(
    (await validateTaskPhaseBaseline({ root: repositoryRoot, ...inputs })).join("\n"),
    /package script .* drifted from checkpoint commands/u,
  );
});

test("baseline validation fails closed when the required checkpoint sequence drifts", async () => {
  const inputs = await baselineInputs();
  inputs.inventory.checkpoint.commands.reverse();
  assert.match(
    (await validateTaskPhaseBaseline({ root: repositoryRoot, ...inputs })).join("\n"),
    /checkpoint commands drifted from the required command sequence/u,
  );
});

test("baseline validation rejects duplicate ids and paths outside the repository", async () => {
  const inputs = await baselineInputs();
  inputs.inventory.harnesses[0].id = inputs.inventory.baselines[0].id;
  inputs.inventory.harnesses[0].path = "../outside";
  const failures = await validateTaskPhaseBaseline({ root: repositoryRoot, ...inputs });
  assert(failures.some((failure) => failure.startsWith("duplicate inventory id:")));
  assert(failures.some((failure) => failure.endsWith("path must stay inside the repository")));
});
