import { readdir, stat } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";

const bundleRoot = path.resolve(
  process.argv[2] ?? "src-tauri/target/universal-apple-darwin/release/bundle",
);

async function onlyArtifact(directory, suffix) {
  const entries = (await readdir(directory)).filter((entry) => entry.endsWith(suffix));
  if (entries.length !== 1) {
    throw new Error(`expected one ${suffix} artifact in ${directory}, found ${entries.length}`);
  }
  return path.join(directory, entries[0]);
}

function check(command, args, label, input) {
  const result = spawnSync(command, args, { encoding: "utf8", input });
  if (result.status !== 0) {
    throw new Error(`${label} failed: ${result.stderr || result.stdout}`);
  }
  return result.stdout.trim();
}

function verifyApp(app) {
  const executableDirectory = path.join(app, "Contents", "MacOS");
  const mainBinary = path.join(executableDirectory, "intercept-proxy");
  const sidecar = path.join(executableDirectory, "intercept-proxy-package-sidecar");
  const plist = path.join(app, "Contents", "Info.plist");
  for (const binary of [mainBinary, sidecar]) {
    check("lipo", [binary, "-verify_arch", "arm64", "x86_64"], `universal binary ${binary}`);
  }
  const identifier = check(
    "plutil",
    ["-extract", "CFBundleIdentifier", "raw", "-o", "-", plist],
    "bundle identifier",
  );
  if (identifier !== "com.interceptproxy.desktop") {
    throw new Error(`unexpected CFBundleIdentifier: ${identifier}`);
  }
  check("codesign", ["--verify", "--deep", "--strict", app], "app code signature");
  return { app, mainBinary, sidecar, identifier };
}

const app = await onlyArtifact(path.join(bundleRoot, "macos"), ".app");
const dmg = await onlyArtifact(path.join(bundleRoot, "dmg"), ".dmg");
for (const file of [
  path.join(app, "Contents", "MacOS", "intercept-proxy"),
  path.join(app, "Contents", "MacOS", "intercept-proxy-package-sidecar"),
  path.join(app, "Contents", "Info.plist"),
  dmg,
]) {
  if (!(await stat(file)).isFile()) throw new Error(`missing file: ${file}`);
}
const built = verifyApp(app);
const attached = check("hdiutil", ["attach", "-plist", "-readonly", "-nobrowse", dmg], "DMG attach");
let mount;
try {
  const json = check("plutil", ["-convert", "json", "-o", "-", "-"], "DMG attach plist", attached);
  const entities = JSON.parse(json)["system-entities"];
  mount = entities.find((entity) => entity["mount-point"])["mount-point"];
  const mountedApp = await onlyArtifact(mount, ".app");
  const mounted = verifyApp(mountedApp);
  console.log(JSON.stringify({ dmg, built, mounted }, null, 2));
} finally {
  if (mount) check("hdiutil", ["detach", mount], "DMG detach");
}
