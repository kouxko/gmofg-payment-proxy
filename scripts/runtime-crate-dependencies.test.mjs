import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import assert from "node:assert/strict";
import { checkCrateDependencies } from "./runtime-crate-dependencies.mjs";

async function violations(crate, dependencies) {
  const root = await mkdtemp(path.join(os.tmpdir(), "crate-boundary-"));
  try {
    const dir = path.join(root, "src-tauri/crates/subject");
    await mkdir(dir, { recursive: true });
    await writeFile(path.join(dir, "Cargo.toml"), `[package]\nname = "${crate}"\n[dependencies]\n${dependencies}\n`);
    return await checkCrateDependencies(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

Deno.test("Android wire crate cannot acquire application or transport dependencies", async () => {
  assert.deepEqual(await violations("intercept-proxy-android-control-protocol", 'serde.workspace = true\nspecta.workspace = true'), []);
  const errors = await violations("intercept-proxy-android-control-protocol", 'intercept-proxy-application = { path = "../application" }\ntokio.workspace = true');
  assert.deepEqual(errors.map((e) => e.code), ["CRATE_DIRECTION", "CRATE_EXTERNAL_DIRECTION"]);
});
Deno.test("traffic projections can consume DTOs but cannot own persistence", async () => {
  assert.deepEqual(await violations("intercept-proxy-traffic-observation", 'intercept-proxy-application = { path = "../application" }\nintercept-proxy-runtime = { path = "../proxy" }\nchrono.workspace = true'), []);
  const errors = await violations("intercept-proxy-traffic-observation", 'intercept-proxy-infrastructure = { path = "../infrastructure" }\nrusqlite.workspace = true');
  assert.deepEqual(errors.map((e) => e.code), ["CRATE_DIRECTION", "CRATE_EXTERNAL_DIRECTION"]);
});
Deno.test("application and runtime cannot depend on outer traffic projections", async () => {
  for (const crate of ["intercept-proxy-application", "intercept-proxy-runtime"]) {
    const errors = await violations(crate, 'intercept-proxy-traffic-observation = { path = "../traffic-observation" }');
    assert.deepEqual(errors.map((e) => e.code), ["CRATE_DIRECTION"]);
  }
});
