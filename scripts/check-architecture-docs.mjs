import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const docsRoot = path.join(repositoryRoot, "docs");
const architectureRoot = path.join(docsRoot, "architecture");

const currentDocuments = [
  "README.md",
  "modules.md",
  "exchange-pipeline.md",
  "data-flow.md",
  "rules-and-protocol-packages.md",
  "runtime-observability.md",
  "security-and-persistence.md",
  "android-vpn-transparent-routing.md",
  "development-guide.md",
];

const mermaidDocuments = new Set([
  "README.md",
  "modules.md",
  "exchange-pipeline.md",
  "data-flow.md",
]);

const decisions = new Map([
  ["decisions/ADR-001-http-socket-boundary.md", { status: "Accepted" }],
  ["decisions/ADR-002-protocol-packages-http.md", { status: "Accepted" }],
  ["decisions/ADR-003-application-zip-ownership.md", { status: "Accepted and implemented" }],
  [
    "decisions/ADR-004-embedded-read-only-mcp.md",
    {
      status: "Superseded by [ADR-008](ADR-008-mcp-environment-configuration.md)",
      supersededBy: "decisions/ADR-008-mcp-environment-configuration.md",
    },
  ],
  ["decisions/ADR-005-runtime-evidence-and-reproduction-report.md", { status: "Accepted" }],
  [
    "decisions/ADR-006-unified-exchange-observation.md",
    {
      status: "Superseded by [ADR-007](ADR-007-exchange-pipeline-runtime-boundary.md) on 2026-08-24",
      supersededBy: "decisions/ADR-007-exchange-pipeline-runtime-boundary.md",
    },
  ],
  [
    "decisions/ADR-007-exchange-pipeline-runtime-boundary.md",
    {
      status: "Accepted",
      supersedes: "decisions/ADR-006-unified-exchange-observation.md",
    },
  ],
  [
    "decisions/ADR-008-mcp-environment-configuration.md",
    {
      status: "Accepted; implementation staged",
      supersedes: "decisions/ADR-004-embedded-read-only-mcp.md",
    },
  ],
]);

const rootDocuments = [
  "README.md",
  "docs/README.md",
  "docs/onboarding-guide.md",
  "docs/requirements.md",
  "docs/user-operation-guide.md",
  "docs/testing/release-validation-matrix.md",
];

const mcpDocuments = [
  "app-integration-guide.md",
  "diagnostic-architecture.md",
  "external-package-integration-guide.md",
  "certificate-concepts.md",
  "tool-reference.md",
];

const forbiddenCurrentDocuments = [
  "data-planes.md",
  "exchange-flow-clean-slate.md",
  "exchange-observation-model.md",
  "lifecycle-persistence-security.md",
  "protocol-boundaries.md",
  "request-lifecycle.md",
  "rules-and-state.md",
  "system-context.md",
  "traceability.md",
  "workspace-and-security.md",
];

async function exists(absolute) {
  try {
    await access(absolute);
    return true;
  } catch {
    return false;
  }
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
  if (!/^flowchart\s+(?:LR|RL|TB|BT|TD)$/u.test(lines[0])) {
    return [`${name}: invalid flowchart declaration`];
  }
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
  const blocks = [...source.matchAll(/```mermaid\s*\n([\s\S]*?)```/gu)]
    .map((match) => match[1].trim().split(/\r?\n/u));
  if (blocks.length === 0) return [`${name}: missing Mermaid source`];
  return blocks.flatMap((lines, index) => {
    const blockName = `${name} Mermaid ${index + 1}`;
    if (lines[0]?.startsWith("flowchart")) return validateFlowchart(blockName, lines);
    if (lines[0] === "sequenceDiagram") return validateSequence(blockName, lines);
    return [`${blockName}: unsupported diagram declaration`];
  });
}

function relativeMarkdownLinks(source) {
  return [...source.matchAll(/\[[^\]]+\]\((?!https?:|#)([^)#]+)(?:#[^)]+)?\)/gu)]
    .map((match) => match[1]);
}

function hasBalancedCodeFences(source) {
  return (source.match(/^```/gmu) ?? []).length % 2 === 0;
}

function referencedRepositoryPaths(source) {
  return [...source.matchAll(/`((?:src(?:-tauri)?|android-companion|scripts|templates|examples|test-support|\.github)\/[A-Za-z0-9_./-]+)`/gu)]
    .map((match) => match[1]);
}

function adrIdentifier(name) {
  return path.basename(name).match(/^(ADR-\d+)/u)?.[1] ?? path.basename(name);
}

function rejectedAlternativesAreExplicit(source) {
  const afterHeading = source.match(/^## Alternatives\s*$([\s\S]*)/mu)?.[1] ?? "";
  const alternatives = afterHeading.split(/^## /mu, 1)[0];
  return /(?:^|\n)\s*-\s*(?:Rejected(?:：|:)|[^\n]+(?:：|:)\s*拒绝(?:，|,|。|\s))/u.test(alternatives);
}

export function validateDecisionPolicy(decisionSources, decisionContracts) {
  const policyFailures = [];
  for (const [name, contract] of decisionContracts) {
    const source = decisionSources.get(name);
    if (source === undefined) continue;

    const statusLines = [...source.matchAll(/^- Status: (.+)$/gmu)].map((match) => match[1]);
    if (statusLines.length !== 1 || statusLines[0] !== contract.status) {
      policyFailures.push(`${name}: expected exact Status: ${contract.status}`);
    }
    if (contract.status.startsWith("Accepted") && !rejectedAlternativesAreExplicit(source)) {
      policyFailures.push(`${name}: rejected alternatives are not explicit`);
    }

    if (contract.supersedes !== undefined) {
      const predecessor = decisionContracts.get(contract.supersedes);
      const predecessorId = adrIdentifier(contract.supersedes);
      const predecessorFile = path.basename(contract.supersedes);
      const reverseLink = `- Supersedes: [${predecessorId}](${predecessorFile})`;
      if (!source.split(/\r?\n/u).includes(reverseLink)) {
        policyFailures.push(`${name}: missing exact reverse link ${reverseLink}`);
      }
      if (predecessor?.supersededBy !== name) {
        policyFailures.push(`${name}: successor relation is not bidirectional with ${contract.supersedes}`);
      }
    }

    if (contract.supersededBy !== undefined) {
      const successor = decisionContracts.get(contract.supersededBy);
      if (successor?.supersedes !== name) {
        policyFailures.push(`${name}: superseded relation is not bidirectional with ${contract.supersededBy}`);
      }
    }
  }
  return policyFailures;
}

async function main() {

const failures = [];
const documents = new Map();

for (const relative of rootDocuments) {
  const absolute = path.join(repositoryRoot, relative);
  if (!(await exists(absolute))) {
    failures.push(`${relative}: required project document is missing`);
    continue;
  }
  const source = await readFile(absolute, "utf8");
  if (!hasBalancedCodeFences(source)) failures.push(`${relative}: unbalanced Markdown code fences`);
  for (const linked of relativeMarkdownLinks(source)) {
    const target = path.resolve(path.dirname(absolute), linked);
    if (!(await exists(target))) failures.push(`${relative}: broken relative link ${linked}`);
  }
  for (const referenced of referencedRepositoryPaths(source)) {
    if (!(await exists(path.join(repositoryRoot, referenced)))) {
      failures.push(`${relative}: missing repository anchor ${referenced}`);
    }
  }
}

const mcpRoot = path.join(docsRoot, "mcp");
const mcpSources = new Map();
for (const name of mcpDocuments) {
  const absolute = path.join(mcpRoot, name);
  if (!(await exists(absolute))) {
    failures.push(`docs/mcp/${name}: required MCP document is missing`);
    continue;
  }
  const source = await readFile(absolute, "utf8");
  mcpSources.set(name, source);
  if (!hasBalancedCodeFences(source)) failures.push(`docs/mcp/${name}: unbalanced Markdown code fences`);
  const lineCount = source.split(/\r?\n/u).length;
  if (lineCount > 500) failures.push(`docs/mcp/${name}: ${lineCount} lines exceeds the 500 line limit`);
  if (!source.startsWith("# ")) failures.push(`docs/mcp/${name}: missing top-level title`);
  for (const linked of relativeMarkdownLinks(source)) {
    const target = path.resolve(path.dirname(absolute), linked);
    if (!(await exists(target))) failures.push(`docs/mcp/${name}: broken relative link ${linked}`);
  }
}

const catalogSource = await readFile(
  path.join(repositoryRoot, "src-tauri/src/mcp/catalog.rs"),
  "utf8",
);
const toolNames = [...catalogSource.matchAll(/\btool\(\s*\n?\s*"([a-z0-9_]+)"/gu)]
  .map((match) => match[1]);
const toolReference = mcpSources.get("tool-reference.md") ?? "";
for (const toolName of toolNames) {
  if (!toolReference.includes(`\`${toolName}\``)) {
    failures.push(`docs/mcp/tool-reference.md: missing public tool ${toolName}`);
  }
}
for (const required of ["成功结果", "错误结果", "additionalProperties", "ExchangeObservationStore"]) {
  if (!toolReference.includes(required)) {
    failures.push(`docs/mcp/tool-reference.md: missing contract term ${required}`);
  }
}
if (toolNames.length !== 36 || new Set(toolNames).size !== toolNames.length) {
  failures.push(`src-tauri/src/mcp/catalog.rs: expected 36 unique documented tools, found ${toolNames.length}`);
}

for (const name of currentDocuments) {
  const absolute = path.join(architectureRoot, name);
  if (!(await exists(absolute))) {
    failures.push(`${name}: required architecture document is missing`);
    continue;
  }
  const source = await readFile(absolute, "utf8");
  documents.set(name, source);
  if (!hasBalancedCodeFences(source)) failures.push(`${name}: unbalanced Markdown code fences`);
  const lineCount = source.split(/\r?\n/u).length;
  if (lineCount > 500) failures.push(`${name}: ${lineCount} lines exceeds the 500 line limit`);
  if (!source.startsWith("# ")) failures.push(`${name}: missing top-level title`);
  if (mermaidDocuments.has(name)) failures.push(...validateMermaid(name, source));
}

for (const name of forbiddenCurrentDocuments) {
  if (await exists(path.join(architectureRoot, name))) {
    failures.push(`${name}: obsolete architecture document must not remain in the current tree`);
  }
}

const decisionSources = new Map();
for (const name of decisions.keys()) {
  const absolute = path.join(architectureRoot, name);
  if (!(await exists(absolute))) {
    failures.push(`${name}: required ADR is missing`);
    continue;
  }
  const source = await readFile(absolute, "utf8");
  decisionSources.set(name, source);
  documents.set(name, source);
}
failures.push(...validateDecisionPolicy(decisionSources, decisions));

for (const [name, source] of documents) {
  for (const linked of relativeMarkdownLinks(source)) {
    const target = path.resolve(architectureRoot, path.dirname(name), linked);
    if (!(await exists(target))) failures.push(`${name}: broken relative link ${linked}`);
  }
  for (const referenced of referencedRepositoryPaths(source)) {
    if (!(await exists(path.join(repositoryRoot, referenced)))) {
      failures.push(`${name}: missing repository anchor ${referenced}`);
    }
  }
}

const templateApi = await readFile(path.join(repositoryRoot, "templates/socket-protocol/API.md"), "utf8");
const templateAuthoring = await readFile(path.join(repositoryRoot, "templates/socket-protocol/AUTHORING.md"), "utf8");
for (const [name, source] of [["API.md", templateApi], ["AUTHORING.md", templateAuthoring]]) {
  for (const required of ["manifest.json", "protocol.js", "display.js", "Sidecar"]) {
    if (!source.includes(required)) failures.push(`templates/socket-protocol/${name}: missing ${required}`);
  }
  if (/^\s*script\s*=/mu.test(source) || /\[(?:document|hooks)\.(?:upstream|downstream)\]/u.test(source)) failures.push(`templates/socket-protocol/${name}: legacy TOML contract is forbidden`);
}

if (failures.length > 0) {
  console.error("Architecture documentation gate failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exitCode = 1;
} else {
  console.log(
    `Architecture documentation gate passed (${currentDocuments.length} current documents, ${decisions.size} ADRs, ${mcpDocuments.length} MCP documents).`,
  );
}
}

if (
  process.argv[1] !== undefined
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  await main();
}
