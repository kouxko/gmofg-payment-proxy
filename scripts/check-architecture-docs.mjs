import { access, readFile } from "node:fs/promises";
import path from "node:path";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const architectureRoot = path.join(repositoryRoot, "docs/architecture");
const baselineDocuments = [
  "system-context.md",
  "protocol-boundaries.md",
  "data-planes.md",
  "lifecycle-persistence-security.md",
  "traceability.md",
];
const decisions = [
  "decisions/ADR-001-http-socket-boundary.md",
  "decisions/ADR-002-protocol-packages-http.md",
  "decisions/ADR-003-application-zip-ownership.md",
];

function section(source, heading) {
  const start = source.indexOf(`## ${heading}`);
  if (start < 0) return "";
  const next = source.indexOf("\n## ", start + 4);
  return source.slice(start, next < 0 ? source.length : next);
}

function balancedDelimiters(source) {
  const pairs = new Map([["]", "["], [")", "("], ["}", "{"]]);
  const stack = [];
  for (const character of source) {
    if (["[", "(", "{"].includes(character)) stack.push(character);
    else if (pairs.has(character) && stack.pop() !== pairs.get(character)) return false;
  }
  return stack.length === 0;
}

function validFlowEndpoint(value) {
  return /^[A-Za-z][A-Za-z0-9_-]*(?:\[[^\n]+\])?$/u.test(value.trim())
    && balancedDelimiters(value);
}

function validateFlowchart(name, lines) {
  const failures = [];
  const stack = [];
  if (!/^flowchart\s+(?:LR|RL|TB|BT|TD)$/u.test(lines[0])) return [`${name}: invalid flowchart declaration`];
  for (const [offset, raw] of lines.slice(1).entries()) {
    const line = raw.trim();
    const lineNumber = offset + 2;
    if (!line || line.startsWith("%%")) continue;
    if (/^subgraph\s+\S/u.test(line)) {
      stack.push("subgraph");
      continue;
    }
    if (line === "end") {
      if (stack.pop() !== "subgraph") failures.push(`${name}:${lineNumber}: unmatched end`);
      continue;
    }
    const edge = line.match(/^(.+?)\s+(-->(?:\|[^|\n]+\|)?|==>|-\.[^\n]+\.->)\s+(.+)$/u);
    if (!edge || !validFlowEndpoint(edge[1]) || !validFlowEndpoint(edge[3])) {
      failures.push(`${name}:${lineNumber}: unsupported flowchart syntax: ${line}`);
    }
  }
  if (stack.length > 0) failures.push(`${name}: unclosed subgraph`);
  return failures;
}

function validateSequence(name, lines) {
  const failures = [];
  const stack = [];
  if (lines[0] !== "sequenceDiagram") return [`${name}: invalid sequenceDiagram declaration`];
  for (const [offset, raw] of lines.slice(1).entries()) {
    const line = raw.trim();
    const lineNumber = offset + 2;
    if (!line || line.startsWith("%%")) continue;
    if (/^participant\s+[A-Za-z][A-Za-z0-9_-]*(?:\s+as\s+\S.*)?$/u.test(line)) continue;
    if (/^(?:alt|opt|loop|par|critical|break)\s+\S/u.test(line)) {
      stack.push(line.split(/\s+/u)[0]);
      continue;
    }
    if (/^(?:else|and)\s+\S/u.test(line)) {
      if (stack.length === 0) failures.push(`${name}:${lineNumber}: branch without open block`);
      continue;
    }
    if (line === "end") {
      if (stack.pop() === undefined) failures.push(`${name}:${lineNumber}: unmatched end`);
      continue;
    }
    if (/^[A-Za-z][A-Za-z0-9_-]*(?:->>|-->>|->|-->)[A-Za-z][A-Za-z0-9_-]*:\s*\S.*$/u.test(line)) continue;
    failures.push(`${name}:${lineNumber}: unsupported sequence syntax: ${line}`);
  }
  if (stack.length > 0) failures.push(`${name}: unclosed sequence block`);
  return failures;
}

function validateMermaid(name, source) {
  const blocks = [...source.matchAll(/```mermaid\s*\n([\s\S]*?)```/gu)].map((match) => match[1].trim().split(/\r?\n/u));
  if (blocks.length === 0) return [`${name}: missing non-empty Mermaid source`];
  return blocks.flatMap((lines, index) => {
    const blockName = `${name} Mermaid ${index + 1}`;
    if (lines[0]?.startsWith("flowchart")) return validateFlowchart(blockName, lines);
    if (lines[0] === "sequenceDiagram") return validateSequence(blockName, lines);
    return [`${blockName}: unsupported diagram declaration`];
  });
}

function validateBaselineSource(name, source) {
  const failures = [];
  for (const heading of ["As-Is", "To-Be", "Open Decision"]) {
    if (!source.includes(`## ${heading}`)) failures.push(`${name}: missing ${heading} section`);
  }
  failures.push(...validateMermaid(name, source));

  const asIs = section(source, "As-Is");
  const evidenceRows = asIs.split("\n").filter((line) => /^\|.*\|$/u.test(line) && !/^\|\s*-+/u.test(line) && !/节点|路径|场景|\| path \|/u.test(line));
  if (evidenceRows.length === 0) failures.push(`${name}: As-Is has no evidence rows`);
  for (const row of evidenceRows) {
    if (!/`(?:src(?:-tauri)?|android-companion)\//u.test(row)) failures.push(`${name}: As-Is row lacks a source anchor: ${row}`);
    if (!/(?:tests?(?:\/|\.)|\.test\.)/u.test(row)) failures.push(`${name}: As-Is row lacks a test anchor: ${row}`);
  }

  const open = section(source, "Open Decision");
  for (const line of open.split("\n").filter((candidate) => /^- /u.test(candidate))) {
    if (!/Owner: R\d{2}[a-z]?(?:\b|[-,])/u.test(line)) failures.push(`${name}: deferred item lacks Owner Rxx: ${line}`);
  }
  return failures;
}

async function exists(absolute) {
  try {
    await access(absolute);
    return true;
  } catch {
    return false;
  }
}

function referencedRepositoryPaths(source) {
  return [...source.matchAll(/`((?:src(?:-tauri)?|android-companion|scripts)\/[A-Za-z0-9_./-]+)`/gu)].map((match) => match[1]);
}

function relativeMarkdownLinks(source) {
  return [...source.matchAll(/\[[^\]]+\]\((?!https?:|#)([^)#]+)(?:#[^)]+)?\)/gu)].map((match) => match[1]);
}

function runFixtures() {
  const valid = `# Fixture\n\n## As-Is\n\n\`\`\`mermaid\nflowchart LR\n A --> B\n\`\`\`\n\n| path | source | test |\n| --- | --- | --- |\n| A -> B | \`src/a.rs\` | \`src/a/tests.rs\` |\n\n## To-Be\n\n- target\n\n## Open Decision\n\n- deferred. Owner: R07a.\n`;
  const fixtures = [
    ["valid baseline", valid, []],
    ["missing Mermaid", valid.replace(/```mermaid[\s\S]+?```/u, ""), ["missing non-empty Mermaid source"]],
    ["missing source path", valid.replace("`src/a.rs`", "none").replace("`src/a/tests.rs`", "tests.rs"), ["lacks a source anchor"]],
    ["missing deferred owner", valid.replace("Owner: R07a.", "later."), ["lacks Owner Rxx"]],
    ["invalid flow declaration", valid.replace("flowchart LR", "flowchart SIDEWAYS"), ["invalid flowchart declaration"]],
    ["unbalanced flow node", valid.replace("A --> B", "A[broken --> B"), ["unsupported flowchart syntax"]],
    ["unknown Mermaid garbage", valid.replace("A --> B", "this is not an edge"), ["unsupported flowchart syntax"]],
    ["balanced subgraph", valid.replace("A --> B", "subgraph Plane\n A --> B\n end"), []],
    ["unclosed subgraph", valid.replace("A --> B", "subgraph Plane\n A --> B"), ["unclosed subgraph"]],
    ["invalid sequence message", valid.replace("flowchart LR\n A --> B", "sequenceDiagram\n participant A\n participant B\n A talks B"), ["unsupported sequence syntax"]],
    ["unclosed sequence alt", valid.replace("flowchart LR\n A --> B", "sequenceDiagram\n participant A\n participant B\n alt branch\n A->>B: request"), ["unclosed sequence block"]],
  ];
  const failures = [];
  for (const [name, source, expectedFragments] of fixtures) {
    const actual = validateBaselineSource(name, source);
    for (const fragment of expectedFragments) {
      if (!actual.some((failure) => failure.includes(fragment))) failures.push(`${name}: fixture did not fail with ${fragment}`);
    }
    if (expectedFragments.length === 0 && actual.length > 0) failures.push(`${name}: fixture unexpectedly failed: ${actual}`);
  }
  return failures;
}

const failures = runFixtures();
const documents = new Map();
for (const name of [...baselineDocuments, ...decisions, "README.md"]) {
  const absolute = path.join(architectureRoot, name);
  if (!(await exists(absolute))) {
    failures.push(`${name}: required architecture file is missing`);
    continue;
  }
  documents.set(name, await readFile(absolute, "utf8"));
}

const designSource = await readFile(path.join(repositoryRoot, "DESIGN.md"), "utf8");
if (!/Status: Active baseline/u.test(designSource) || /Status: Draft/u.test(designSource)) {
  failures.push("DESIGN.md: architecture baseline must be active, not Draft");
}
if (!designSource.includes("## Open decisions and deferred delivery")) {
  failures.push("DESIGN.md: missing explicit open/deferred delivery section");
}

for (const name of baselineDocuments) {
  const source = documents.get(name);
  if (!source) continue;
  failures.push(...validateBaselineSource(name, source));
  for (const referenced of referencedRepositoryPaths(source)) {
    if (!(await exists(path.join(repositoryRoot, referenced)))) failures.push(`${name}: missing repository anchor ${referenced}`);
  }
}

for (const name of decisions) {
  const source = documents.get(name);
  if (!source) continue;
  if (!/Status: Accepted/u.test(source)) failures.push(`${name}: decision is not explicitly accepted`);
  if (!/Rejected/u.test(source)) failures.push(`${name}: rejected alternatives are not explicit`);
  if (!/(?:Open items|implementation deferred|future)/iu.test(source)) failures.push(`${name}: open/deferred state is not explicit`);
}

for (const [name, source] of documents) {
  for (const linked of relativeMarkdownLinks(source)) {
    if (!(await exists(path.resolve(architectureRoot, path.dirname(name), linked)))) failures.push(`${name}: broken relative link ${linked}`);
  }
}

if (!failures.some((failure) => failure.includes("fixture"))) {
  console.log("Architecture documentation fixtures passed (11 cases, including Mermaid syntax failures).");
}
if (failures.length > 0) {
  console.error("Architecture documentation gate failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exitCode = 1;
} else {
  console.log(`Architecture documentation gate passed (${baselineDocuments.length} baselines, ${decisions.length} ADRs).`);
}
