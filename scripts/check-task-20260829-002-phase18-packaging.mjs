import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(process.env.PHASE18_CHECK_ROOT ?? ".");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const config = JSON.parse(read("src-tauri/tauri.conf.json"));
const packageJson = JSON.parse(read("package.json"));
const stage = read("scripts/stage-package-sidecar.mjs");
const build = read("scripts/build-macos-universal.mjs");
const verify = read("scripts/verify-macos-universal.mjs");
const sign = read("scripts/sign-macos-app.mjs");
const runner = read("scripts/e2e_macos_mounted_release.py");
const legacyE2e = read("scripts/e2e_external_packages.py");
const workflow = read(".github/workflows/windows-release.yml");
const compositionRoot = read("src-tauri/src/lib.rs");
const rootCargo = read("src-tauri/Cargo.toml");
const proxyCargo = read("src-tauri/crates/proxy/Cargo.toml");
const failures = [];
const requireText = (owner, source, token) => {
  if (!source.includes(token)) failures.push(`${owner} missing ${token}`);
};

if (JSON.stringify(config.bundle?.externalBin) !== JSON.stringify(["binaries/intercept-proxy-package-sidecar"])) {
  failures.push("Tauri externalBin must use the target-suffixed Boa sidecar stem");
}
if (packageJson.scripts["build:macos:universal"] !== "node scripts/build-macos-universal.mjs") {
  failures.push("package script must own the canonical macOS Universal build");
}
if (packageJson.scripts["verify:macos:universal"] !== "node scripts/verify-macos-universal.mjs") {
  failures.push("package script must own macOS Universal verification");
}
for (const token of ["cargo", "build", "--release", "--target", "intercept-proxy-package-sidecar-${target}"]) {
  requireText("sidecar staging", stage, token);
}
for (const token of ["aarch64-apple-darwin", "x86_64-apple-darwin", "universal-apple-darwin", '"app"', "sign-macos-app.mjs", "macos-universal-vendored-openssl", "verify-macos-universal.mjs"]) {
  requireText("Universal build", build, token);
}
requireText(
  "Universal build",
  build,
  'for (const target of ["aarch64-apple-darwin", "x86_64-apple-darwin"])',
);
requireText("Universal sidecar merge", build, 'run("lipo", [\n  "-create",');
for (const token of ["lipo", '"-create"', "intercept-proxy-package-sidecar-universal-apple-darwin", '"-verify_arch"', "arm64", "x86_64"]) {
  requireText("Universal sidecar merge", build, token);
}
for (const token of ["diskutil", '"image"', '"create"', '"from"', '"--volumeName"', '"--format"', '"UDZO"']) {
  requireText("signed App DMG creation", build, token);
}
if (/run\("hdiutil",\s*\[\s*"create"/u.test(build)) {
  failures.push("signed App DMG creation must not use deprecated hdiutil create");
}
for (const token of ["lipo", "arm64", "x86_64", "Info.plist", "CFBundleIdentifier", "codesign", "--deep", "--strict", ".dmg", "intercept-proxy-package-sidecar", "hdiutil", "-readonly", "mountedApp"]) {
  requireText("Universal verification", verify, token);
}
requireText("Universal verification", verify, "for (const binary of [mainBinary, sidecar])", "both main and sidecar must be lipo-verified");
for (const token of ["codesign", "--force", "--deep", "--sign", '"-"']) {
  requireText("ad-hoc app sealing", sign, token);
}
for (const token of ['"hdiutil", "attach"', "-readonly", "-nobrowse", "CFFIXED_USER_HOME", "environment_candidate_create", "environment_candidate_apply", "iso8583-ascii-standard", "http_byte_chain", "socket_byte_chain", "osascript", "orphaned bundled sidecar", "intercept-proxy.sqlite3"]) {
  requireText("mounted release E2E", runner, token);
}
for (const token of ["node scripts/stage-package-sidecar.mjs x86_64-pc-windows-msvc", "pnpm build:macos:universal"]) {
  requireText("release workflow", workflow, token);
}
const windowsBuild = workflow.slice(
  workflow.indexOf("  build:\n"),
  workflow.indexOf("  build-macos:\n"),
);
const windowsSidecarStage = windowsBuild.indexOf(
  "node scripts/stage-package-sidecar.mjs x86_64-pc-windows-msvc",
);
const windowsTauriBuild = windowsBuild.indexOf("- name: Build MSI and NSIS installers");
if (windowsSidecarStage < 0 || windowsTauriBuild < 0 || windowsSidecarStage >= windowsTauriBuild) {
  failures.push("Windows build job must stage its x86_64 sidecar before Tauri packaging");
}
requireText("Schema100 E2E", legacyE2e, "EXPECTED_SCHEMA_VERSION = 100");
requireText("Release composition root", compositionRoot, "use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};");
if (/#\[cfg\(debug_assertions\)\]\s*use intercept_proxy_host::\{ApplicationHostBuilder, HostPlatformServices\};/u.test(compositionRoot)) {
  failures.push("Release composition imports must not be debug-only");
}
requireText("root Cargo", rootCargo, 'macos-universal-vendored-openssl = ["intercept-proxy-runtime/macos-universal-vendored-openssl"]');
requireText("proxy Cargo", proxyCargo, 'macos-universal-vendored-openssl = ["openssl/vendored"]');
if (/Expected database schema 19|version != \(19,\)/u.test(legacyE2e)) {
  failures.push("current E2E must not accept the removed Schema19 database");
}

if (failures.length) {
  failures.forEach((failure) => console.error(`FAIL: ${failure}`));
  process.exit(1);
}
console.log("PASS: Phase18 Universal packaging and mounted release E2E contract");
