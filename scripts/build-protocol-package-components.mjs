import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const targetRoot = path.join(repositoryRoot, "src-tauri", "target", "protocol-package-components");
const distributionRoot = path.join(repositoryRoot, "dist", "protocol-package-components");
const cargo = process.env.CARGO ?? "cargo";
const runNativeTests = process.argv.includes("--test");

const manifests = [
  ...(await discoverCargoManifests(path.join(repositoryRoot, "templates"), (relative) =>
    relative.endsWith("Cargo.toml")
  )),
  ...(await discoverCargoManifests(path.join(repositoryRoot, "examples"), (relative) =>
    relative.endsWith("component/Cargo.toml")
  )),
].sort();

if (manifests.length === 0) {
  throw new Error("no protocol-package Component manifests were discovered");
}

await rm(distributionRoot, { recursive: true, force: true });
await mkdir(distributionRoot, { recursive: true });

const index = [];
const packageNames = new Set();
for (const manifest of manifests) {
  const cargoToml = await readFile(manifest, "utf8");
  const packageName = readPackageName(cargoToml, manifest);
  if (packageNames.has(packageName)) {
    throw new Error(`duplicate protocol-package Component package name: ${packageName}`);
  }
  packageNames.add(packageName);
  const packageTarget = path.join(targetRoot, packageName);
  if (runNativeTests) {
    await run(cargo, [
      "test",
      "--locked",
      "--all-targets",
      "--manifest-path",
      manifest,
    ]);
  }
  await run(cargo, [
    "build",
    "--locked",
    "--release",
    "--target",
    "wasm32-wasip2",
    "--target-dir",
    packageTarget,
    "--manifest-path",
    manifest,
  ]);

  const artifactName = `${packageName.replaceAll("-", "_")}.wasm`;
  const artifact = path.join(packageTarget, "wasm32-wasip2", "release", artifactName);
  const rawComponent = await readFile(artifact);
  const manifestBytes = await readFile(path.join(path.dirname(manifest), "manifest.json"));
  JSON.parse(manifestBytes.toString("utf8"));
  const bytes = appendCustomSection(
    rawComponent,
    "intercept-proxy:manifest",
    manifestBytes,
  );
  validateComponent(bytes, artifact);
  const outputName = `${packageName}.wasm`;
  await writeFile(path.join(distributionRoot, outputName), bytes);
  index.push({
    package: packageName,
    source: path.relative(repositoryRoot, manifest),
    artifact: path.posix.join("dist/protocol-package-components", outputName),
    bytes: bytes.length,
  });
}

await writeFile(
  path.join(distributionRoot, "index.json"),
  `${JSON.stringify({ target: "wasm32-wasip2", components: index }, null, 2)}\n`,
);

console.log(`${runNativeTests ? "Tested and built" : "Built"} ${index.length} protocol-package Components:`);
for (const component of index) {
  console.log(`- ${component.source} -> ${component.artifact} (${component.bytes} bytes)`);
}

async function discoverCargoManifests(root, accepts) {
  const discovered = [];
  await walk(root, "", discovered, accepts);
  return discovered;
}

async function walk(root, relative, discovered, accepts) {
  const current = path.join(root, relative);
  for (const entry of await readdir(current, { withFileTypes: true })) {
    if (entry.name === "target" || entry.name === "dist" || entry.name.startsWith(".")) continue;
    const childRelative = path.join(relative, entry.name);
    if (entry.isDirectory()) {
      await walk(root, childRelative, discovered, accepts);
    } else if (entry.isFile() && accepts(childRelative)) {
      discovered.push(path.join(root, childRelative));
    }
  }
}

function readPackageName(cargoToml, manifest) {
  const packageStart = cargoToml.indexOf("[package]");
  if (packageStart < 0) throw new Error(`cannot find [package] in ${manifest}`);
  const afterPackage = cargoToml.slice(packageStart + "[package]".length);
  const nextSection = afterPackage.search(/\n\[/);
  const packageSection = nextSection < 0 ? afterPackage : afterPackage.slice(0, nextSection);
  const packageName = packageSection?.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!packageName) throw new Error(`cannot read [package].name from ${manifest}`);
  return packageName;
}

function validateComponent(bytes, artifact) {
  const componentHeader = Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00]);
  if (bytes.length < componentHeader.length || !bytes.subarray(0, 8).equals(componentHeader)) {
    throw new Error(`${artifact} is not a WebAssembly Component`);
  }
  if (!bytes.includes(Buffer.from("intercept-proxy:manifest"))) {
    throw new Error(`${artifact} does not contain intercept-proxy:manifest`);
  }
}

function appendCustomSection(component, name, data) {
  const nameBytes = Buffer.from(name, "utf8");
  const payload = Buffer.concat([encodeUnsignedLeb128(nameBytes.length), nameBytes, data]);
  return Buffer.concat([
    component,
    Buffer.from([0]),
    encodeUnsignedLeb128(payload.length),
    payload,
  ]);
}

function encodeUnsignedLeb128(value) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`cannot encode unsigned LEB128 value: ${value}`);
  }
  const bytes = [];
  do {
    let byte = value & 0x7f;
    value = Math.floor(value / 128);
    if (value !== 0) byte |= 0x80;
    bytes.push(byte);
  } while (value !== 0);
  return Buffer.from(bytes);
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: repositoryRoot, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} exited with code ${code ?? "null"} signal ${signal ?? "none"}`));
    });
  });
}
