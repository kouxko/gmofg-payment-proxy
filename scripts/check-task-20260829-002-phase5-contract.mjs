import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");
const fail = (message) => { throw new Error(`TASK-20260829-002 Phase5 contract: ${message}`); };
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const stripComments = (source) => source
  .replace(/\/\*[\s\S]*?\*\//gu, " ")
  .replace(/\/\/[^\n]*/gu, " ");
const codeOnly = (source) => stripComments(source)
  .replace(/"(?:\\.|[^"\\])*"/gu, '""')
  .replace(/'(?:\\.|[^'\\])*'/gu, "''");

function walk(directory, predicate) {
  const files = [];
  for (const entry of fs.readdirSync(path.join(root, directory), { withFileTypes: true })) {
    const relative = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...walk(relative, predicate));
    else if (predicate(relative)) files.push(relative);
  }
  return files;
}

function closingDelimiter(source, opening, open, close) {
  let depth = 0;
  for (let index = opening; index < source.length; index += 1) {
    if (source[index] === open) depth += 1;
    else if (source[index] === close && --depth === 0) return index;
  }
  return -1;
}

function enumDefinitions(source) {
  const clean = stripComments(source);
  const definitions = [];
  for (const match of clean.matchAll(/\b(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)\s*\{/gu)) {
    const opening = match.index + match[0].lastIndexOf("{");
    const closing = closingDelimiter(clean, opening, "{", "}");
    if (closing < 0) continue;
    const prefix = clean.slice(Math.max(0, match.index - 500), match.index);
    definitions.push({
      symbol: match[1],
      body: clean.slice(opening + 1, closing).replace(/\s+/gu, " ").trim(),
      attributes: prefix.slice(prefix.lastIndexOf("\n\n") + 2),
    });
  }
  return definitions;
}

const wireShapes = new Map([
  ["ConditionTree", {
    tag: /serde\s*\(\s*tag\s*=\s*"operator"[\s\S]*content\s*=\s*"children"/u,
    variants: [/\bAll\s*\(\s*Vec\s*<\s*ConditionTree\s*>\s*\)/u, /\bAny\s*\(\s*Vec\s*<\s*ConditionTree\s*>\s*\)/u, /\bLeaf\s*\(\s*Condition\s*\)/u],
  }],
  ["DocumentPredicate", {
    tag: /serde\s*\(\s*tag\s*=\s*"type"[\s\S]*content\s*=\s*"value"/u,
    variants: [/\bString\s*\(\s*StringPredicate\s*\)/u, /\bNumber\s*\(\s*NumberPredicate\s*\)/u, /\bBoolean\s*\(\s*BooleanPredicate\s*\)/u, /\bNullEqual\b/u],
  }],
  ["DocumentMutation", {
    tag: /serde\s*\(\s*tag\s*=\s*"type"/u,
    variants: [/\bSet\s*\{/u, /\bClear\s*\{/u, /\bInsert\s*\{/u, /\bAppend\s*\{/u, /\bJsonPointer\b/u, /\bDocumentValue\b/u],
  }],
  ["UnifiedAction", {
    tag: /serde\s*\(\s*tag\s*=\s*"source"[\s\S]*content\s*=\s*"value"/u,
    variants: [/\bRecordMatch\b/u, /\bDocument\s*\(\s*DocumentMutation\s*\)/u, /\bHttp\s*\(\s*RuleAction\s*\)/u, /\bTerminal\s*\(\s*TerminalAction\s*\)/u],
  }],
]);

function isSerdeSpectaWire(definition, shape) {
  const derive = definition.attributes.match(/derive\s*\(([^)]*)\)/su)?.[1] ?? "";
  return ["Serialize", "Deserialize", "Type"].every((trait) => new RegExp(`\\b${trait}\\b`, "u").test(derive))
    && shape.tag.test(definition.attributes)
    && shape.variants.every((variant) => variant.test(definition.body));
}

function extractGeneratedType(source, symbol) {
  const marker = `export type ${symbol} =`;
  const start = source.indexOf(marker);
  if (start < 0) fail(`generated semantic drift: missing ${symbol}`);
  const bodyStart = start + marker.length;
  const nextExport = source.indexOf("\nexport ", bodyStart);
  const raw = source.slice(bodyStart, nextExport < 0 ? source.length : nextExport).trim();
  return raw.replace(/\/\*[\s\S]*?\*\//gu, " ").replace(/;\s*$/u, "").replace(/\s+/gu, " ").trim();
}

function command(commandName, args) {
  const result = spawnSync(commandName, args, { cwd: root, encoding: "utf8" });
  if (result.status !== 0) fail(`test discovery command failed: ${commandName} ${args.join(" ")}\n${result.stderr}`);
  return result.stdout;
}

function discoverTests() {
  if (process.env.PHASE5_DISCOVERY_JSON) return JSON.parse(process.env.PHASE5_DISCOVERY_JSON);
  const rustOutput = command("cargo", [
    "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-domain",
    "--test", "phase5_unified_rule_domain", "--", "--list", "--format", "terse",
  ]);
  const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
  const tsOutput = command(pnpm, [
    "exec", "vitest", "list", "src/features/rules/rule-definition-model.test.ts", "--json",
  ]);
  return {
    rust: rustOutput.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6)).sort(),
    typescript: JSON.parse(tsOutput).map((test) => test.name).sort(),
  };
}

const fixturePath = "test-support/fixtures/task-20260829-002/phase-5/unified-rule-domain/contract-inventory.json";
if (!fs.existsSync(path.join(root, fixturePath))) fail(`missing active fixture ${fixturePath}`);
const fixture = JSON.parse(read(fixturePath));
if (fixture.task_id !== "TASK-20260829-002" || fixture.case_id !== "phase5-unified-rule-domain") fail("active fixture identity drift");
const rustTestPath = fixture.discoverable_tests?.rust;
const tsTestPath = fixture.discoverable_tests?.typescript;
for (const file of [rustTestPath, tsTestPath]) {
  if (typeof file !== "string" || !fs.existsSync(path.join(root, file))) fail(`missing discoverable test ${file}`);
}
const rustTests = stripComments(read(rustTestPath));
const tsTests = stripComments(read(tsTestPath));
if (/#\[(?:ignore|should_panic)\]|\.(?:skip|only)\s*\(/u.test(`${rustTests}\n${tsTests}`)) fail("ignored, skipped, or only-focused contract test detected");
const discovered = discoverTests();
for (const [kind, expected] of [
  ["rust", fixture.discoverable_tests.rust_names],
  ["typescript", fixture.discoverable_tests.typescript_names],
]) {
  const actual = [...(discovered[kind] ?? [])].sort();
  const wanted = [...(expected ?? [])].sort();
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) fail(`${kind === "rust" ? "Cargo" : "Vitest"} discovered tests drift: expected=${wanted.length}, actual=${actual.length}`);
}
for (const testName of fixture.discoverable_tests.rust_names) {
  if (!new RegExp(`\\bfn\\s+${testName}\\s*\\(`, "u").test(rustTests)) fail(`Cargo discovered test missing real source definition: ${testName}`);
}

const domainRustFiles = walk("src-tauri/crates/domain/src", (file) => file.endsWith(".rs"));
const canonicalFiles = new Map([
  ["ConditionTree", "src-tauri/crates/domain/src/unified_rule_execution.rs"],
  ["DocumentPredicate", "src-tauri/crates/domain/src/unified_rule_execution.rs"],
  ["DocumentMutation", "src-tauri/crates/domain/src/unified_rule_execution.rs"],
  ["UnifiedAction", "src-tauri/crates/domain/src/unified_rule_execution.rs"],
]);
const allDefinitions = domainRustFiles.flatMap((file) => enumDefinitions(read(file)).map((definition) => ({ file, ...definition })));
for (const [symbol, canonicalFile] of canonicalFiles) {
  const named = allDefinitions.filter((definition) => definition.symbol === symbol);
  if (named.length !== 1 || named[0].file !== canonicalFile) fail(`canonical unified wire symbol uniqueness drift: ${symbol}`);
  const shape = wireShapes.get(symbol);
  if (!isSerdeSpectaWire(named[0], shape)) fail(`canonical unified wire shape drift: ${symbol}`);
  const shaped = allDefinitions.filter((definition) => isSerdeSpectaWire(definition, shape));
  if (shaped.length !== 1) fail(`second unified wire owner: ${symbol}`);
}
for (const file of domainRustFiles) {
  const code = stripComments(read(file));
  for (const symbol of canonicalFiles.keys()) {
    const aliases = [...code.matchAll(new RegExp(`\\btype\\s+(\\w+)\\s*=\\s*${symbol}\\s*;`, "gu"))];
    if (aliases.length > 0) fail(`alias unified wire owner: ${aliases[0][1]} -> ${symbol}`);
  }
}

const unifiedRuleOwner = stripComments(read("src-tauri/crates/domain/src/unified_rule.rs"));
if (/\bpub\s+conditions\s*:\s*Vec\s*<\s*(?:MatchCondition|DocumentCondition)\s*>/u.test(unifiedRuleOwner)) {
  fail("flat condition owner detected");
}
if (/\bpub\s+actions\s*:\s*Vec\s*<\s*(?:RuleAction|DocumentAction)\s*>/u.test(unifiedRuleOwner)) {
  fail("parallel action owner detected");
}

const generatedGolden = JSON.parse(read(fixture.generated_wire_golden));
if (generatedGolden.semantic_sha256 !== sha256(JSON.stringify(generatedGolden.type_blocks))) fail("generated golden hash drift");
const generatedSource = read("src/generated/rust-types.ts");
const actualGenerated = Object.fromEntries(Object.keys(generatedGolden.type_blocks).map((symbol) => [symbol, extractGeneratedType(generatedSource, symbol)]));
if (JSON.stringify(actualGenerated) !== JSON.stringify(generatedGolden.type_blocks)
    || sha256(JSON.stringify(actualGenerated)) !== generatedGolden.semantic_sha256) fail("generated semantic drift");

const productionRoots = ["src-tauri/crates/domain/src", "src-tauri/crates/application/src", "src-tauri/crates/infrastructure/src", "src-tauri/src", "src"];
const productionFiles = productionRoots.flatMap((directory) => walk(directory, (file) => {
  if (!/\.(?:rs|ts|tsx)$/u.test(file)) return false;
  return !/(?:^|\/)(?:tests|requirements_tests)(?:\/|$)|(?:_tests|\.test)\.(?:rs|ts|tsx)$/u.test(file);
}));
for (const file of productionFiles) {
  const source = codeOnly(read(file));
  for (const match of source.matchAll(/(?:\bsort_by(?:_key)?|\.sort)\s*\(/gu)) {
    const opening = match.index + match[0].lastIndexOf("(");
    const closing = closingDelimiter(source, opening, "(", ")");
    const comparator = source.slice(opening, closing < 0 ? source.length : closing + 1);
    if (/\bcreat(?:ed|ion)_order\b/u.test(comparator)) fail(`created_order runtime/read comparator detected in ${file}`);
  }
}

const legacySymbols = new Set(["DocumentCondition", "DocumentAction", "MatchCondition", "RuleAction"]);
const allowlist = fixture.phase12_legacy_owner_allowlist;
if (!Array.isArray(allowlist) || allowlist.length !== legacySymbols.size) fail("Phase12 allowlist must contain exact file+symbol+reason entries");
const allowed = new Map();
for (const entry of allowlist) {
  if (!entry || typeof entry.file !== "string" || !entry.file.endsWith(".rs")
      || !legacySymbols.has(entry.symbol) || typeof entry.reason !== "string"
      || !entry.reason.startsWith("Phase 12 removes ")) fail("Phase12 allowlist entry requires exact file+symbol+reason");
  const key = `${entry.file}#${entry.symbol}`;
  if (allowed.has(key)) fail(`duplicate Phase12 allowlist entry ${key}`);
  allowed.set(key, false);
}
for (const definition of allDefinitions.filter((item) => legacySymbols.has(item.symbol))) {
  const key = `${definition.file}#${definition.symbol}`;
  if (!allowed.has(key)) fail(`legacy owner escaped exact Phase12 allowlist: ${key}`);
  if (allowed.get(key)) fail(`legacy owner count exceeds one in allowlisted file: ${key}`);
  allowed.set(key, true);
}
for (const [key, used] of allowed) {
  if (!used) fail(`stale Phase12 allowlist ${key}`);
}

console.log(`TASK-20260829-002 Phase5 contract PASS (Cargo=${discovered.rust.length}, Vitest=${discovered.typescript.length})`);
