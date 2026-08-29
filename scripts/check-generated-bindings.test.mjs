import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, unlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { checkGeneratedBindings } from "./check-generated-bindings.mjs";

async function withGeneratedFixture(initial, body) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "intercept-proxy-bindings-"));
  const generatedPath = path.join(directory, "rust-types.ts");
  await writeFile(generatedPath, initial);
  try {
    await body(generatedPath);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("accepts fresh deterministic generated bindings", async () => {
  await withGeneratedFixture("fresh\n", async (generatedPath) => {
    const failures = await checkGeneratedBindings({
      generatedPath,
      runGenerator: () => writeFile(generatedPath, "fresh\n"),
    });
    assert.deepEqual(failures, []);
    assert.equal(await readFile(generatedPath, "utf8"), "fresh\n");
  });
});

test("reports stale bindings and restores the checked-in bytes", async () => {
  await withGeneratedFixture("stale\n", async (generatedPath) => {
    const failures = await checkGeneratedBindings({
      generatedPath,
      runGenerator: () => writeFile(generatedPath, "fresh\n"),
    });
    assert.deepEqual(failures, ["src/generated/rust-types.ts is stale; run pnpm bindings"]);
    assert.equal(await readFile(generatedPath, "utf8"), "stale\n");
  });
});

test("reports nondeterministic output and restores the checked-in bytes", async () => {
  await withGeneratedFixture("checked-in\n", async (generatedPath) => {
    let generation = 0;
    const failures = await checkGeneratedBindings({
      generatedPath,
      runGenerator: () => writeFile(generatedPath, `generated-${generation += 1}\n`),
    });
    assert.deepEqual(failures, [
      "src/generated/rust-types.ts is stale; run pnpm bindings",
      "binding generation is not deterministic across consecutive runs",
    ]);
    assert.equal(await readFile(generatedPath, "utf8"), "checked-in\n");
  });
});

test("restores checked-in bytes when the generator fails", async () => {
  await withGeneratedFixture("checked-in\n", async (generatedPath) => {
    await assert.rejects(
      checkGeneratedBindings({
        generatedPath,
        runGenerator: async () => {
          await writeFile(generatedPath, "partial\n");
          throw new Error("generator failed");
        },
      }),
      /generator failed/u,
    );
    assert.equal(await readFile(generatedPath, "utf8"), "checked-in\n");
  });
});

test("restores checked-in bytes and preserves the generator error when output is deleted", async () => {
  await withGeneratedFixture("checked-in\n", async (generatedPath) => {
    const generatorError = new Error("generator deleted output");
    await assert.rejects(
      checkGeneratedBindings({
        generatedPath,
        runGenerator: async () => {
          await unlink(generatedPath);
          throw generatorError;
        },
      }),
      (error) => error === generatorError,
    );
    assert.equal(await readFile(generatedPath, "utf8"), "checked-in\n");
  });
});
