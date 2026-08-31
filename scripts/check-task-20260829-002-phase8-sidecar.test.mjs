import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const root = process.cwd();
const checker = path.join(root, "scripts/check-task-20260829-002-phase8-sidecar.mjs");
const files = [
  "src-tauri/Cargo.toml",
  "src-tauri/crates/package-runtime/Cargo.toml",
  "src-tauri/crates/package-runtime/src/lib.rs",
  "src-tauri/crates/package-runtime/src/sidecar.rs",
  "src-tauri/crates/package-runtime/src/bin/intercept-proxy-package-sidecar.rs",
  "src-tauri/crates/package-runtime/tests/phase8_sidecar_runtime.rs",
  "src-tauri/crates/package-runtime/tests/phase8_sidecar_review.rs",
  "scripts/check-task-20260829-002-phase7-package-runtime.mjs",
  "src-tauri/tauri.conf.json",
];
const discovery = JSON.stringify({
  runtime: [
    "all_eight_fixed_exports_map_to_the_matching_direction",
    "fixed_exports_are_cached_after_registration_precheck",
    "http_hooks_receive_and_return_unicode_strings_without_socket_frame_exports",
    "missing_or_non_callable_fixed_exports_fail_before_registration",
    "only_package_relative_esm_specifiers_are_accepted",
    "relative_esm_modules_are_evaluated_once_and_exports_are_cached",
    "required_exports_are_prechecked_without_calling_package_code",
    "socket_base64_is_presented_to_javascript_as_uint8array_and_returns_canonical_base64",
    "socket_encode_rejects_non_uint8array_results",
  ],
  review: [
    "dynamic_import_uses_boa_loader_and_evaluates_the_lazy_module_once",
    "nested_parent_imports_and_static_cycles_evaluate_each_module_once",
    "relative_imports_cannot_escape_the_package_root",
  ],
});

function sandbox() {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "phase8-sidecar-"));
  for (const file of files) {
    const destination = path.join(target, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(root, file), destination);
  }
  return target;
}

function run(cwd, discovered = discovery) {
  return spawnSync(process.execPath, [checker], {
    cwd,
    encoding: "utf8",
    env: cwd === root ? process.env : { ...process.env, PHASE8_CHECKER_TEST_MODE: "sandbox", PHASE8_DISCOVERY_JSON: discovered },
  });
}

function replace(file, before, after) {
  return (target) => {
    const name = path.join(target, file);
    const source = fs.readFileSync(name, "utf8");
    const next = typeof before === "string" ? source.split(before).join(after) : source.replace(before, after);
    fs.writeFileSync(name, next);
  };
}

function append(file, text) {
  return (target) => fs.appendFileSync(path.join(target, file), `\n${text}\n`);
}

test("canonical repository passes", () => {
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

for (const [name, mutate, expected] of [
  ["missing Boa dependency", replace(files[0], "boa_engine", "removed_engine"), /Boa dependency/u],
  ["disabled Boa default features", replace(files[0], 'boa_engine = "=0.22.0"', 'boa_engine = { version = "=0.22.0", default-features = false }'), /default features/u],
  ["missing sidecar bin", replace(files[1], "[[bin]]", "[[example]]"), /binary target/u],
  ["missing module loader", replace(files[3], "PackageModuleLoader", "NoModuleLoader"), /module lifecycle/u],
  ["bare ESM imports", replace(files[3], 'specifier.starts_with("./")', 'specifier.starts_with("pkg:")'), /relative ESM/u],
  ["missing HTTP export branch", replace(files[3], "HTTP_PROTOCOL_EXPORTS", "SOCKET_PROTOCOL_EXPORTS"), /distinct fixed contracts/u],
  ["missing export cache", replace(files[3], "exports: BTreeMap", "exports: Vec"), /cached/u],
  ["missing Promise settlement", replace(files[3], "value.as_promise()", "None"), /Promise hook results/u],
  ["missing callable check", replace(files[3], "is_callable", "is_not_callable"), /export validation/u],
  ["missing fixed export", replace(files[3], "upstreamDisplay", "upstreamRender"), /fixed Sidecar export/u],
  ["missing Uint8Array input boundary", replace(files[3], "JsUint8Array::from_iter", "JsArray::from_iter"), /Uint8Array/u],
  ["missing Uint8Array output boundary", replace(files[3], "JsUint8Array::from_object", "JsArray::from_object"), /Uint8Array/u],
  ["Proxy global Host binding injection", append(files[3], 'fn inject(context: &mut Context) { context.register_global_builtin_callable(js_string!("process"), 0, NativeFunction::from_fn_ptr(bad)); }'), /Host binding injection/u],
  ["Proxy NativeFunction Host binding", append(files[3], "fn inject() { let _ = NativeFunction::from_fn_ptr(process_binding); }"), /Host binding injection/u],
  ["Proxy custom HostHooks", append(files[3], "struct NodeHostHooks; impl HostHooks for NodeHostHooks {}"), /Host binding injection/u],
  ["timeout policy", append(files[3], "struct HookPolicy { rpc_timeout: u64 }"), /hook timeout/u],
  ["Rhai fallback", append(files[3], "fn bad() { let _ = rhai::Engine::new(); }"), /Rhai\/TOML/u],
  ["phase7 checker owns phase8", append(files[7], "const SIDE = \"intercept-proxy-package-sidecar\";"), /Phase7 checker/u],
  ["missing Tauri externalBin", replace(files[8], '"externalBin": [', '"removedExternalBin": ['), /externalBin/u],
  ["wrong Tauri externalBin stem", replace(files[8], "binaries/intercept-proxy-package-sidecar", "binaries/wrong-sidecar"), /externalBin/u],
]) {
  test(`fails closed for ${name}`, () => {
    const target = sandbox();
    mutate(target);
    const result = run(target);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
  });
}

test("fails closed for Cargo zero-test discovery", () => {
  const target = sandbox();
  const result = run(target, "[]");
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Cargo discovered Phase8/u);
});
