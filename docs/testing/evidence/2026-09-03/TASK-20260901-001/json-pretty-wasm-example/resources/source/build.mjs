import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const packageRoot = path.dirname(fileURLToPath(import.meta.url));
const manifestPath = path.join(packageRoot, "Cargo.toml");
const targetRoot = path.join(packageRoot, "target");
const distributionRoot = path.join(packageRoot, "dist");
const cargo = process.env.CARGO ?? "cargo";

await run(cargo, ["test", "--locked", "--all-targets", "--manifest-path", manifestPath]);
await run(cargo, [
  "build",
  "--locked",
  "--release",
  "--target",
  "wasm32-wasip2",
  "--manifest-path",
  manifestPath,
]);

const compiledPath = path.join(
  targetRoot,
  "wasm32-wasip2",
  "release",
  "intercept_proxy_json_pretty_component.wasm",
);
const manifest = await readFile(path.join(packageRoot, "manifest.json"));
validateManifest(JSON.parse(manifest.toString("utf8")));
const component = appendCustomSection(
  await readFile(compiledPath),
  "intercept-proxy:manifest",
  manifest,
);
validateComponent(component);

await mkdir(distributionRoot, { recursive: true });
const outputPath = path.join(distributionRoot, "json-pretty-1.0.0.wasm");
await writeFile(outputPath, component);
const digest = createHash("sha256").update(component).digest("hex");
await writeFile(`${outputPath}.sha256`, `${digest}  ${path.basename(outputPath)}\n`);
console.log(`${path.relative(packageRoot, outputPath)} ${component.length} bytes sha256 ${digest}`);

function appendCustomSection(component, name, data) {
  const nameBytes = Buffer.from(name, "utf8");
  const payload = Buffer.concat([encodeUnsignedLeb128(nameBytes.length), nameBytes, data]);
  return Buffer.concat([component, Buffer.from([0]), encodeUnsignedLeb128(payload.length), payload]);
}

function encodeUnsignedLeb128(value) {
  const bytes = [];
  do {
    let byte = value & 0x7f;
    value = Math.floor(value / 128);
    if (value !== 0) byte |= 0x80;
    bytes.push(byte);
  } while (value !== 0);
  return Buffer.from(bytes);
}

function validateComponent(bytes) {
  const header = Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00]);
  if (!bytes.subarray(0, header.length).equals(header)) {
    throw new Error("compiled artifact is not a WebAssembly Component");
  }

  const manifestSections = [];
  let offset = header.length;
  while (offset < bytes.length) {
    const sectionId = bytes[offset];
    offset += 1;
    const sectionSize = decodeUnsignedLeb128(bytes, offset);
    offset = sectionSize.nextOffset;
    const sectionEnd = offset + sectionSize.value;
    if (sectionEnd > bytes.length) throw new Error("compiled Component contains a truncated section");
    if (sectionId === 0) {
      const nameSize = decodeUnsignedLeb128(bytes, offset);
      const nameStart = nameSize.nextOffset;
      const nameEnd = nameStart + nameSize.value;
      if (nameEnd > sectionEnd) throw new Error("compiled Component contains a truncated custom section name");
      const name = bytes.subarray(nameStart, nameEnd).toString("utf8");
      if (name === "intercept-proxy:manifest") {
        manifestSections.push(bytes.subarray(nameEnd, sectionEnd));
      }
    }
    offset = sectionEnd;
  }
  if (manifestSections.length !== 1) {
    throw new Error(`compiled Component must contain exactly one top-level intercept-proxy:manifest section; found ${manifestSections.length}`);
  }
  if (!manifestSections[0].equals(manifest)) {
    throw new Error("embedded intercept-proxy:manifest differs from manifest.json");
  }
}

function validateManifest(value) {
  const expectedIdentity = value?.api === 1
    && value?.kind === "http"
    && value?.package?.id === "json-pretty"
    && value?.package?.version === "1.0.0"
    && typeof value?.package?.name === "string"
    && value.package.name.trim().length > 0;
  if (!expectedIdentity) throw new Error("manifest.json does not describe json-pretty@1.0.0 HTTP API 1");
  for (const direction of ["upstream", "downstream"]) {
    const documentDirection = value?.document?.[direction];
    if (!isPlainObject(documentDirection) || Object.keys(documentDirection).length !== 0) {
      throw new Error(`manifest.json document.${direction} must not declare a schema`);
    }
  }
}

function isPlainObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function decodeUnsignedLeb128(bytes, startOffset) {
  let value = 0;
  let shift = 0;
  let offset = startOffset;
  while (offset < bytes.length && shift < 35) {
    const byte = bytes[offset];
    offset += 1;
    value += (byte & 0x7f) * (2 ** shift);
    if ((byte & 0x80) === 0) return { value, nextOffset: offset };
    shift += 7;
  }
  throw new Error("compiled Component contains an invalid unsigned LEB128 value");
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: packageRoot, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} exited with code ${code ?? "null"} signal ${signal ?? "none"}`));
    });
  });
}
