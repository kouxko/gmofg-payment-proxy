import { readFile, readdir } from "node:fs/promises";
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const root = path.resolve(import.meta.dirname, "..");
const inventoryPath = path.join(root, "test-support/fixtures/task-20260829-002/phase-4/package-contract/inventory.json");
const execFileAsync = promisify(execFile);

const sha256 = (value) => createHash("sha256").update(value).digest("hex");

function record(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : undefined;
}

function exactKeys(value, required, optional = []) {
  const object = record(value);
  return object !== undefined
    && required.every((key) => key in object)
    && Object.keys(object).every((key) => required.includes(key) || optional.includes(key));
}

function validSchema(value, contract) {
  const schema = record(value);
  if (!schema || !contract.schemaTypes.includes(schema.type)) return false;
  if (schema.title !== undefined && (typeof schema.title !== "string"
    || schema.title.trim() === "" || Array.from(schema.title).length > contract.schemaTitleMaxChars)) return false;
  if (["string", "number", "boolean"].includes(schema.type)) return exactKeys(schema, ["type"], ["title"]);
  if (schema.type === "object") {
    const properties = record(schema.properties);
    return exactKeys(schema, ["type", "properties"], ["title"])
      && properties !== undefined && Object.values(properties).every((child) => validSchema(child, contract));
  }
  return exactKeys(schema, ["type", "items"], ["title"]) && validSchema(schema.items, contract);
}

function validDocument(value) {
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value)
    && (!Number.isInteger(value) || Number.isSafeInteger(value));
  if (Array.isArray(value)) return value.every(validDocument);
  const object = record(value);
  return object !== undefined && Object.values(object).every(validDocument);
}

function validVersion(value, contract) {
  if (!(new RegExp(contract.packageVersionPattern, "u")).test(value)
    || value.length > contract.packageVersionMaxBytes) return false;
  const maximum = BigInt(contract.packageVersionCoreNumericMax);
  return value.split(/[+-]/u, 1)[0].split(".").every((part) => BigInt(part) <= maximum);
}

const canonicalBase64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u;

function validValueKind(value, kind, snapshot) {
  if (kind === "string") return typeof value === "string";
  if (kind === "document") return validDocument(value);
  if (kind === "canonicalBase64") return typeof value === "string" && canonicalBase64.test(value);
  if (kind === "frameResult") return validateFrameResultSnapshot(value, snapshot);
  return false;
}

function validateFrameResultSnapshot(value, snapshot) {
  const result = record(value);
  const contract = snapshot.frameResult;
  if (!result || result[contract.discriminator] === undefined) return false;
  const variant = contract.variants.find((entry) => entry.status === result.status);
  if (!variant || !exactKeys(result, variant.required, variant.optional)) return false;
  if (variant.status === "need_more" && result.requiredBytes !== undefined) {
    return Number.isSafeInteger(result.requiredBytes)
      && result.requiredBytes >= variant.requiredBytesMinimum;
  }
  if (variant.status === "need_more") return true;
  if (variant.status === "complete") {
    return Number.isSafeInteger(result.consumedBytes)
      && result.consumedBytes >= variant.consumedBytesMinimum;
  }
  return variant.status === "reject" && typeof result.reason === "string";
}

export function validateManifestSnapshot(value, snapshot) {
  const manifest = record(value);
  const contract = snapshot.manifest;
  if (!manifest || !exactKeys(manifest, contract.required)) return false;
  if (manifest.api !== contract.api || !contract.kinds.includes(manifest.kind)) return false;
  const metadata = record(manifest.package);
  if (!metadata || !exactKeys(metadata, contract.packageRequired)
    || !contract.packageRequired.every((key) => typeof metadata[key] === "string")) return false;
  if (!(new RegExp(contract.packageIdPattern, "u")).test(metadata.id)
    || metadata.id.length > contract.packageIdMaxBytes
    || !validVersion(metadata.version, contract)
    || (contract.packageNameVisible && metadata.name.trim() === "")) return false;
  const document = record(manifest.document);
  if (!document || !exactKeys(document, contract.documentRequired)) return false;
  for (const directionName of ["upstream", "downstream"]) {
    const direction = record(document[directionName]);
    if (!direction || !exactKeys(direction, [], contract.directionProperties)) return false;
    if (direction.schema !== undefined && !validSchema(direction.schema, contract)) return false;
  }
  return manifest.kind !== "socket" || !contract.socketRequiresBothSchemas
    || (record(document.upstream).schema !== undefined && record(document.downstream).schema !== undefined);
}

export function validateRegistrationSnapshot(value, snapshot) {
  const notification = record(value);
  return notification !== undefined
    && exactKeys(notification, snapshot.registration.required)
    && notification.jsonrpc === snapshot.jsonRpc
    && notification.method === snapshot.registration.method
    && !(snapshot.registration.idAllowed || "id" in notification)
    && snapshot.registration.paramsKind === "manifest"
    && validateManifestSnapshot(notification.params, snapshot);
}

export function validateRequestSnapshot(value, snapshot) {
  const request = record(value);
  if (!request || !exactKeys(request, snapshot.requestEnvelope.required)
    || request.jsonrpc !== snapshot.jsonRpc || typeof request.id !== snapshot.requestEnvelope.idKind) return false;
  const method = snapshot.requests.find((entry) => entry.method === request.method);
  const params = record(request.params);
  if (!method || !params || !exactKeys(params, method.params.required)) return false;
  return Object.entries(method.params.properties)
    .every(([field, kind]) => validValueKind(params[field], kind, snapshot));
}

export function validateSuccessSnapshot(value, method, snapshot) {
  const response = record(value);
  return response !== undefined
    && exactKeys(response, snapshot.successEnvelope.required)
    && response.jsonrpc === snapshot.jsonRpc
    && typeof response.id === snapshot.successEnvelope.idKind
    && validValueKind(response.result, method.resultKind, snapshot);
}

export function validateFailureSnapshot(value, snapshot) {
  const response = record(value);
  if (!response || !exactKeys(response, snapshot.failureEnvelope.required)
    || response.jsonrpc !== snapshot.jsonRpc || typeof response.id !== snapshot.failureEnvelope.idKind) return false;
  const error = record(response.error);
  const data = record(error?.data);
  return error !== undefined && exactKeys(error, snapshot.failureEnvelope.errorRequired)
    && Number.isSafeInteger(error.code) && typeof error.message === snapshot.failureEnvelope.messageKind
    && data !== undefined && exactKeys(data, snapshot.failureEnvelope.dataRequired)
    && snapshot.stableCodes.includes(data.code);
}

export function validateCanonicalGoldenSnapshot(value, snapshot) {
  const golden = record(value);
  if (!golden || !exactKeys(golden, ["manifest", "registration", "requests", "successes", "failure"])
    || !validateManifestSnapshot(golden.manifest, snapshot)
    || !validateRegistrationSnapshot(golden.registration, snapshot)
    || !Array.isArray(golden.requests) || golden.requests.length !== snapshot.requests.length
    || !golden.requests.every((request) => validateRequestSnapshot(request, snapshot))) return false;
  const methodsById = new Map(golden.requests.map((request) => [request.id, snapshot.requests.find((entry) => entry.method === request.method)]));
  if (new Set(golden.requests.map((request) => request.method)).size !== snapshot.requests.length) return false;
  const successes = record(golden.successes);
  if (!successes || !exactKeys(successes, ["frame", "decode", "encode", "display"])
    || !Array.isArray(successes.frame)) return false;
  const responses = [...successes.frame, successes.decode, successes.encode, successes.display];
  return responses.every((response) => {
    const method = methodsById.get(response?.id);
    return method !== undefined && validateSuccessSnapshot(response, method, snapshot);
  }) && validateFailureSnapshot(golden.failure, snapshot);
}

function mcpSemanticFailures(snapshot, validation, goldenRaw) {
  const expectedRequests = [
    ["hooks.upstream.frame", { buffer: "canonicalBase64" }, "frameResult"],
    ["hooks.downstream.frame", { buffer: "canonicalBase64" }, "frameResult"],
    ["hooks.upstream.decode", { input: "string" }, "document"],
    ["hooks.downstream.decode", { input: "string" }, "document"],
    ["hooks.upstream.encode", { originalInput: "string", document: "document" }, "string"],
    ["hooks.downstream.encode", { originalInput: "string", document: "document" }, "string"],
    ["document.upstream.display", { document: "document" }, "string"],
    ["document.downstream.display", { document: "document" }, "string"],
  ].map(([method, properties, resultKind]) => ({
    method,
    params: { required: Object.keys(properties), properties, additionalProperties: false },
    resultKind,
  }));
  const exact = (actual, expected) => JSON.stringify(actual) === JSON.stringify(expected);
  const failures = [];
  if (!exact(Object.keys(snapshot), ["schemaVersion", "jsonRpc", "manifest", "registration", "requests", "requestEnvelope", "successEnvelope", "failureEnvelope", "frameResult", "stableCodes", "canonicalGoldenSha256"])) failures.push("MCP top-level semantic contract drift");
  if (snapshot.schemaVersion !== 1 || snapshot.jsonRpc !== "2.0") failures.push("MCP JSON-RPC semantic contract drift");
  const manifest = snapshot.manifest;
  if (!exact(manifest, {
    required: ["api", "kind", "package", "document"], additionalProperties: false, api: 1,
    kinds: ["http", "socket"], packageRequired: ["id", "version", "name", "description"],
    packageAdditionalProperties: false, packageIdPattern: validation.packageIdPattern,
    packageIdMaxBytes: validation.packageIdMaxBytes, packageVersionPattern: validation.packageVersionPattern,
    packageVersionMaxBytes: validation.packageVersionMaxBytes,
    packageVersionCoreNumericMax: validation.packageVersionCoreNumericMax, packageNameVisible: true,
    documentRequired: ["upstream", "downstream"], directionProperties: ["schema"],
    schemaTypes: ["string", "number", "boolean", "object", "array"],
    schemaTitleMaxChars: validation.schemaTitleMaxChars, socketRequiresBothSchemas: true,
  })) failures.push("MCP Manifest semantic contract drift");
  if (!exact(snapshot.registration, { required: ["jsonrpc", "method", "params"], additionalProperties: false, method: "package.register", idAllowed: false, paramsKind: "manifest" })) failures.push("MCP registration semantic contract drift");
  if (!exact(snapshot.requests, expectedRequests)) failures.push("MCP request/result semantic contract drift");
  if (!exact(snapshot.requestEnvelope, { required: ["jsonrpc", "id", "method", "params"], additionalProperties: false, idKind: "string" })) failures.push("MCP request envelope semantic contract drift");
  if (!exact(snapshot.successEnvelope, { required: ["jsonrpc", "id", "result"], additionalProperties: false, idKind: "string" })) failures.push("MCP success envelope semantic contract drift");
  if (!exact(snapshot.failureEnvelope, { required: ["jsonrpc", "id", "error"], additionalProperties: false, idKind: "string", errorRequired: ["code", "message", "data"], errorAdditionalProperties: false, errorCodeKind: "integer", messageKind: "string", dataRequired: ["code"], dataAdditionalProperties: false })) failures.push("MCP failure semantic contract drift");
  if (!exact(snapshot.frameResult, { discriminator: "status", variants: [
    { status: "need_more", required: ["status"], optional: ["requiredBytes"], requiredBytesMinimum: 0 },
    { status: "complete", required: ["status", "consumedBytes"], optional: [], consumedBytesMinimum: 1 },
    { status: "reject", required: ["status", "reason"], optional: [] },
  ], additionalProperties: false })) failures.push("MCP FrameResult semantic contract drift");
  if (!exact(snapshot.stableCodes, validation.stableErrorCodes)) failures.push("MCP stable-code semantic contract drift");
  if (snapshot.canonicalGoldenSha256 !== sha256(goldenRaw)) failures.push("MCP canonical golden semantic contract drift");
  return failures;
}

function closingBrace(source, opening) {
  let depth = 0;
  for (let index = opening; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    else if (source[index] === "}" && --depth === 0) return index;
  }
  return -1;
}

function blankRange(characters, source, start, end) {
  for (let index = start; index < end; index += 1) {
    if (source[index] !== "\n" && source[index] !== "\r") characters[index] = " ";
  }
}

function quotedEnd(source, opening, quote) {
  for (let index = opening + 1; index < source.length; index += 1) {
    if (source[index] === "\\") index += 1;
    else if (source[index] === quote) return index + 1;
    else if (source[index] === "\n" && quote === "'") return -1;
  }
  return -1;
}

function characterLiteralEnd(source, opening) {
  const end = quotedEnd(source, opening, "'");
  if (end === -1) return -1;
  const value = source.slice(opening + 1, end - 1);
  return value.startsWith("\\") || Array.from(value).length === 1 ? end : -1;
}

function maskRustNoise(source, preserveLiterals = false) {
  const characters = source.split("");
  let attributeDepth = 0;
  for (let index = 0; index < source.length;) {
    if (attributeDepth > 0) {
      if (source[index] === "\"") {
        const end = quotedEnd(source, index, "\"");
        index = end === -1 ? source.length : end;
      } else {
        if (source[index] === "[") attributeDepth += 1;
        else if (source[index] === "]") attributeDepth -= 1;
        index += 1;
      }
      continue;
    }
    if (source.startsWith("#[", index)) {
      attributeDepth = 1;
      index += 2;
      continue;
    }
    if (source.startsWith("//", index)) {
      const end = source.indexOf("\n", index + 2);
      blankRange(characters, source, index, end === -1 ? source.length : end);
      index = end === -1 ? source.length : end;
      continue;
    }
    if (source.startsWith("/*", index)) {
      let depth = 1;
      let end = index + 2;
      while (end < source.length && depth > 0) {
        if (source.startsWith("/*", end)) { depth += 1; end += 2; }
        else if (source.startsWith("*/", end)) { depth -= 1; end += 2; }
        else end += 1;
      }
      blankRange(characters, source, index, end);
      index = end;
      continue;
    }
    const raw = source.slice(index).match(/^(?:br|r)(#*)"/u);
    if (raw) {
      const terminator = `"${raw[1]}`;
      const closing = source.indexOf(terminator, index + raw[0].length);
      const end = closing === -1 ? source.length : closing + terminator.length;
      if (!preserveLiterals) blankRange(characters, source, index, end);
      index = end;
      continue;
    }
    const quote = source[index] === "\"" ? "\"" : source[index] === "b" && source[index + 1] === "\"" ? "\"" : undefined;
    if (quote) {
      const opening = source[index] === "b" ? index + 1 : index;
      const end = quotedEnd(source, opening, quote);
      if (!preserveLiterals) blankRange(characters, source, index, end === -1 ? source.length : end);
      index = end === -1 ? source.length : end;
      continue;
    }
    if (source[index] === "'") {
      const end = characterLiteralEnd(source, index);
      if (end !== -1) {
        if (!preserveLiterals) blankRange(characters, source, index, end);
        index = end;
        continue;
      }
    }
    index += 1;
  }
  return characters.join("");
}

function attributesBefore(source, declarationIndex) {
  return source.slice(0, declarationIndex).match(/((?:#\[[^\]]*\]\s*)+)$/u)?.[1] ?? "";
}

function namedBlocks(source, kind) {
  const blocks = [];
  const declarations = new RegExp(`\\b(?:pub(?:\\([^)]*\\))?\\s+)?${kind}\\s+(\\w+)[^;{]*\\{`, "gu");
  for (const declaration of source.matchAll(declarations)) {
    const opening = declaration.index + declaration[0].lastIndexOf("{");
    const closing = closingBrace(source, opening);
    if (closing !== -1) blocks.push({
      name: declaration[1],
      attributes: attributesBefore(source, declaration.index),
      body: source.slice(opening + 1, closing),
    });
  }
  return blocks;
}

function topLevelParts(source) {
  const parts = [];
  let start = 0;
  let angle = 0;
  let round = 0;
  let square = 0;
  let brace = 0;
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (character === "<") angle += 1;
    else if (character === ">") angle = Math.max(0, angle - 1);
    else if (character === "(") round += 1;
    else if (character === ")") round = Math.max(0, round - 1);
    else if (character === "[") square += 1;
    else if (character === "]") square = Math.max(0, square - 1);
    else if (character === "{") brace += 1;
    else if (character === "}") brace = Math.max(0, brace - 1);
    else if (character === "," && angle === 0 && round === 0 && square === 0 && brace === 0) {
      parts.push(source.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(source.slice(start));
  return parts;
}

function parsedFields(body) {
  const fields = [];
  for (const part of topLevelParts(body)) {
    const match = part.trim().match(/^((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*([\s\S]+)$/u);
    if (match) fields.push({ attributes: match[1], name: match[2], type: match[3].trim() });
  }
  return fields;
}

function derivedFor(attributes, direction) {
  for (const derive of attributes.matchAll(/#\[\s*derive\s*\(([^)]*)\)\s*\]/gu)) {
    if (new RegExp(`(?:^|[^\\w])(?:serde::)?${direction}(?:$|[^\\w])`, "u").test(derive[1])) return true;
  }
  return false;
}

function traitImplBlocks(source, traitPattern, bodySource = source) {
  const blocks = [];
  const implementations = new RegExp(`\\bimpl(?:\\s*<[^>{}]*>)?\\s+${traitPattern}(?:\\s*<[^>]*>)?\\s+for\\s+(\\w+)[^;{]*\\{`, "gu");
  for (const implementation of source.matchAll(implementations)) {
    const opening = implementation.index + implementation[0].lastIndexOf("{");
    const closing = closingBrace(source, opening);
    if (closing !== -1) blocks.push({ name: implementation[1], body: bodySource.slice(opening + 1, closing) });
  }
  return blocks;
}

function manualSerializeFields(source, literalSource) {
  const fields = new Map();
  for (const implementation of traitImplBlocks(source, "(?:serde::)?Serialize", literalSource)) {
    const names = new Set();
    for (const field of implementation.body.matchAll(/\bserialize_(?:field|entry)\s*\(\s*"([^"]+)"/gu)) names.add(field[1]);
    fields.set(implementation.name, names);
  }
  return fields;
}

function referencedConstantFields(source, body) {
  const fields = new Set();
  const constants = /\bconst\s+(\w+)\s*:[^=;]+=\s*&?\[([\s\S]*?)\]\s*;/gu;
  for (const constant of source.matchAll(constants)) {
    if (!new RegExp(`\\b${constant[1]}\\b`, "u").test(body)) continue;
    for (const value of constant[2].matchAll(/"([^"]+)"/gu)) fields.add(value[1]);
  }
  return fields;
}

function acceptedMatchFields(body) {
  const fields = new Set();
  for (const field of body.matchAll(/"([^"]+)"\s*(?=\||=>)/gu)) fields.add(field[1]);
  return fields;
}

function manualDeserializeFields(source, literalSource) {
  const fields = new Map();
  const visitors = traitImplBlocks(source, "(?:serde::de::)?Visitor", literalSource);
  for (const implementation of traitImplBlocks(source, "(?:serde::)?Deserialize", literalSource)) {
    const names = new Set([...acceptedMatchFields(implementation.body), ...referencedConstantFields(literalSource, implementation.body)]);
    const visitorNames = new Set(
      [...implementation.body.matchAll(/\b([A-Za-z_]\w*Visitor)\b/gu)].map((match) => match[1]),
    );
    for (const visitor of visitors) {
      if (!visitorNames.has(visitor.name)) continue;
      for (const name of acceptedMatchFields(visitor.body)) names.add(name);
    }
    fields.set(implementation.name, names);
  }
  return fields;
}

function localFieldType(type, structures) {
  const identifiers = [...type.matchAll(/\b[A-Za-z_]\w*\b/gu)].map((match) => match[0]);
  return identifiers.reverse().find((identifier) => structures.has(identifier));
}

function fieldNames(field, direction) {
  if (/\bskip\b/u.test(field.attributes)
    || (direction === "Serialize" && /\bskip_serializing\b/u.test(field.attributes))
    || (direction === "Deserialize" && /\bskip_deserializing\b/u.test(field.attributes))) return [];
  const renamed = field.attributes.match(/\brename\s*=\s*"([^"]+)"/u)?.[1];
  const directional = field.attributes.match(new RegExp(`\\b${direction === "Serialize" ? "serialize" : "deserialize"}\\s*=\\s*"([^"]+)"`, "u"))?.[1];
  const names = [directional ?? renamed ?? field.name];
  if (direction === "Deserialize") {
    for (const alias of field.attributes.matchAll(/\balias\s*=\s*"([^"]+)"/gu)) names.push(alias[1]);
  }
  return names;
}

function effectiveTypeFields(type, direction, structures, derived, manual, visited) {
  if (manual[direction].has(type)) return manual[direction].get(type);
  if (!derived[direction].has(type) || visited.has(type) || !structures.has(type)) return new Set();
  return effectiveFields(
    structures.get(type).fields,
    direction,
    structures,
    derived,
    manual,
    new Set(visited).add(type),
  );
}

function effectiveFields(fields, direction, structures, derived, manual, visited) {
  const names = new Set();
  for (const field of fields) {
    if (/\bflatten\b/u.test(field.attributes)) {
      const referenced = localFieldType(field.type, structures);
      if (!referenced) continue;
      for (const name of effectiveTypeFields(referenced, direction, structures, derived, manual, visited)) names.add(name);
    } else {
      for (const name of fieldNames(field, direction)) names.add(name);
    }
  }
  return names;
}

function ownsManifest(fields) {
  return ["api", "kind", "package", "document"].every((field) => fields.has(field));
}

function manifestWireOwners(rawSource) {
  const source = maskRustNoise(rawSource);
  const manualSource = maskRustNoise(rawSource, true);
  const structBlocks = namedBlocks(source, "struct").map((block) => ({ ...block, fields: parsedFields(block.body) }));
  const structures = new Map(structBlocks.map((block) => [block.name, block]));
  const derived = { Serialize: new Set(), Deserialize: new Set() };
  const manual = {
    Serialize: manualSerializeFields(source, manualSource),
    Deserialize: manualDeserializeFields(source, manualSource),
  };
  for (const direction of ["Serialize", "Deserialize"]) {
    for (const structure of structBlocks) {
      if (derivedFor(structure.attributes, direction)) derived[direction].add(structure.name);
    }
  }
  const owners = [];
  for (const direction of ["Serialize", "Deserialize"]) {
    for (const [type, fields] of manual[direction]) {
      if (ownsManifest(fields)) owners.push({ symbol: type });
    }
  }
  for (const structure of structBlocks) {
    for (const direction of ["Serialize", "Deserialize"]) {
      if (!derived[direction].has(structure.name)) continue;
      if (ownsManifest(effectiveTypeFields(structure.name, direction, structures, derived, manual, new Set()))) {
        owners.push({ symbol: structure.name });
        break;
      }
    }
  }
  for (const enumeration of namedBlocks(source, "enum")) {
    const enumEligible = ["Serialize", "Deserialize"].filter((direction) =>
      derivedFor(enumeration.attributes, direction));
    if (enumEligible.length === 0 || !/\buntagged\b/u.test(enumeration.attributes)) continue;
    for (const part of topLevelParts(enumeration.body)) {
      const variant = part.trim().match(/^(?:#\[[^\]]*\]\s*)*(\w+)\s*\{([\s\S]*)\}$/u);
      if (!variant) continue;
      const fields = parsedFields(variant[2]);
      if (enumEligible.some((direction) => ownsManifest(effectiveFields(fields, direction, structures, derived, manual, new Set([enumeration.name]))))) {
        owners.push({ symbol: `${enumeration.name}::${variant[1]}` });
      }
    }
  }
  return owners;
}

async function filesBelow(directory, extension) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.name === "target") continue;
    const child = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await filesBelow(child, extension));
    else if (entry.name.endsWith(extension)) files.push(child);
  }
  return files;
}

export async function discoverRequiredCargoTests(targets) {
  const discovered = {};
  for (const target of Object.keys(targets)) {
    const { stdout } = await execFileAsync("cargo", [
      "test", "--manifest-path", "src-tauri/Cargo.toml", "-p",
      "intercept-proxy-package-contract", "--test", target, "--", "--list", "--format", "terse",
    ], { cwd: root });
    discovered[target] = stdout.split("\n")
      .filter((line) => line.endsWith(": test"))
      .map((line) => line.slice(0, -": test".length));
  }
  return discovered;
}

export async function checkPhase4({ inventory, read = readFile, discoveredTests } = {}) {
  const active = inventory ?? JSON.parse(await read(inventoryPath, "utf8"));
  const failures = [];
  const targets = active.required_test_targets ?? {};
  if (Object.keys(targets).length !== 5) failures.push("Phase 4 must declare exactly five required integration targets");
  let cargoTests = discoveredTests;
  try {
    cargoTests ??= await discoverRequiredCargoTests(targets);
  } catch (error) {
    failures.push(`Cargo test discovery failed: ${error.message}`);
    cargoTests = {};
  }
  for (const [target, names] of Object.entries(targets)) {
    if (!Array.isArray(names) || names.length === 0) {
      failures.push(`${target}: required test names must be nonzero`);
      continue;
    }
    const actual = cargoTests[target];
    if (!Array.isArray(actual) || actual.length === 0) failures.push(`${target}: Cargo discovered zero tests`);
    for (const name of names) {
      if (!actual?.includes(name)) failures.push(`${target}: Cargo did not discover required test ${name}`);
    }
  }
  const cargo = await read(path.join(root, "src-tauri/crates/package-contract/Cargo.toml"), "utf8");
  if (!cargo.includes("intercept-proxy-domain = { path = \"../domain\" }")) failures.push("contract crate must depend on Domain");
  for (const forbidden of ["intercept-proxy-application", "intercept-proxy-infrastructure", "intercept-proxy-runtime", "intercept-proxy-protocol-scripting"])
    if (cargo.includes(forbidden)) failures.push(`contract crate has forbidden internal dependency ${forbidden}`);
  const domainCargo = await read(path.join(root, "src-tauri/crates/domain/Cargo.toml"), "utf8");
  if (domainCargo.includes("package-contract")) failures.push("Domain must not depend back on package-contract");
  const rustFiles = await filesBelow(path.join(root, "src-tauri"), ".rs");
  const legacySymbol = /pub\s+(?:struct|enum|type)\s+(External(?:DocumentWire|Package(?:MethodSuffix|Direction|MethodNamespace|Metadata|DocumentDirection|Documents|DirectionHooks|Hooks|Registration)|Frame(?:Request|Result)|Decode(?:Request|Response)|Encode(?:Request|Response)|Display(?:Request|Response)))\b/gu;
  const allowlist = active.phase7_legacy_wire_allowlist ?? [];
  const allowedLegacyDefinitions = new Map();
  for (const entry of allowlist) {
    if (!exactKeys(entry, ["file", "symbol", "reason"])
      || typeof entry.file !== "string" || !entry.file.endsWith(".rs")
      || typeof entry.symbol !== "string" || !entry.symbol.startsWith("External")
      || typeof entry.reason !== "string" || entry.reason.trim() === "") {
      failures.push("Phase 7 legacy allowlist entries require exact .rs file, symbol and precise reason");
      continue;
    }
    const key = `${entry.file}#${entry.symbol}`;
    if (allowedLegacyDefinitions.has(key)) failures.push(`${key}: duplicate Phase 7 legacy allowlist entry`);
    allowedLegacyDefinitions.set(key, false);
  }
  const allowedManifestOwners = new Map();
  for (const entry of active.phase7_manifest_owner_allowlist ?? []) {
    if (!exactKeys(entry, ["file", "symbol", "reason"])
      || typeof entry.file !== "string" || !entry.file.endsWith(".rs")
      || typeof entry.symbol !== "string" || typeof entry.reason !== "string" || entry.reason.trim() === "") {
      failures.push("Phase 7 Manifest owner allowlist entries require exact .rs file, symbol and precise reason");
      continue;
    }
    allowedManifestOwners.set(`${entry.file}#${entry.symbol}`, false);
  }
  for (const file of rustFiles) {
    const source = await read(file, "utf8");
    const relative = path.relative(root, file).split(path.sep).join("/");
    if (!file.includes("/crates/package-contract/") && !relative.includes("/tests/")) {
      for (const owner of manifestWireOwners(source)) {
        const key = `${relative}#${owner.symbol}`;
        if (!allowedManifestOwners.has(key)) {
          const reason = owner.flatten
            ? "serde(flatten) can conceal a second package Manifest-shaped wire owner"
            : "second package Manifest-shaped wire owner";
          failures.push(`${key}: ${reason}`);
        }
        else allowedManifestOwners.set(key, true);
      }
    }
    for (const match of source.matchAll(legacySymbol)) {
      const key = `${relative}#${match[1]}`;
      if (!allowedLegacyDefinitions.has(key)) failures.push(`${key}: unexpected Phase 7 legacy wire definition`);
      else allowedLegacyDefinitions.set(key, true);
    }
  }
  for (const [key, used] of allowedLegacyDefinitions)
    if (!used) failures.push(`${key}: stale or unused Phase 7 legacy allowlist entry`);
  for (const [key, used] of allowedManifestOwners)
    if (!used) failures.push(`${key}: stale or unused Phase 7 Manifest owner allowlist entry`);
  const generated = await read(path.join(root, "src/generated/rust-types.ts"), "utf8");
  if (typeof active.generated_sha256 !== "string" || sha256(generated) !== active.generated_sha256)
    failures.push("generated binding is not the exact recorded Rust export");
  for (const fragment of active.generated_required_fragments ?? [])
    if (!generated.includes(fragment)) failures.push(`generated binding missing ${fragment}`);
  for (const stale of active.generated_forbidden_types ?? [])
    if (new RegExp(`export type ${stale}\\b`, "u").test(generated)) failures.push(`generated binding contains forbidden stale type ${stale}`);
  const validationMatch = generated.match(/export const PACKAGE_CONTRACT_VALIDATION = (\{.*\}) as const;/u);
  let validation;
  try { validation = JSON.parse(validationMatch?.[1] ?? ""); }
  catch { failures.push("generated binding validation metadata is missing or invalid"); }
  const schemaRaw = await read(path.join(root, "src-tauri/src/mcp/tests/fixtures/package_contract_api1/schema.snapshot.json"), "utf8");
  if (typeof active.mcp_snapshot_sha256 !== "string" || sha256(schemaRaw) !== active.mcp_snapshot_sha256)
    failures.push("MCP package schema snapshot is not the exact recorded contract");
  const schema = JSON.parse(schemaRaw);
  const canonicalGoldenRaw = await read(path.join(root, "test-support/fixtures/task-20260829-002/phase-4/package-contract/golden.json"), "utf8");
  const goldenRaw = await read(path.join(root, "src-tauri/src/mcp/tests/fixtures/package_contract_api1/golden.json"), "utf8");
  if (goldenRaw !== canonicalGoldenRaw) failures.push("MCP golden differs from the canonical Rust/TS golden");
  if (validation) failures.push(...mcpSemanticFailures(schema, validation, goldenRaw));
  try {
    if (!validateCanonicalGoldenSnapshot(JSON.parse(goldenRaw), schema)) failures.push("MCP canonical golden does not satisfy the complete schema");
  } catch {
    failures.push("MCP canonical golden or schema is invalid");
  }
  for (const copy of active.evidence_byte_copies ?? []) {
    if (!exactKeys(copy, ["source", "evidence", "sha256"]) || typeof copy.sha256 !== "string") {
      failures.push("evidence copy inventory entries require exact source, evidence and SHA-256");
      continue;
    }
    try {
      const [source, evidence] = await Promise.all([
        read(path.join(root, copy.source)), read(path.join(root, copy.evidence)),
      ]);
      if (sha256(source) !== copy.sha256 || sha256(evidence) !== copy.sha256
        || !Buffer.from(source).equals(Buffer.from(evidence))) {
        failures.push(`${copy.evidence}: evidence resource SHA/bytes differ from ${copy.source}`);
      }
    } catch {
      failures.push(`${copy.evidence}: required evidence byte copy is missing`);
    }
  }
  return failures;
}

async function main() {
  const failures = await checkPhase4();
  if (failures.length) {
    for (const failure of failures) process.stderr.write(`FAIL ${failure}\n`);
    process.exitCode = 1;
  } else process.stdout.write("PASS TASK-20260829-002 Phase 4 package contract inventory and parity checks\n");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) await main();
