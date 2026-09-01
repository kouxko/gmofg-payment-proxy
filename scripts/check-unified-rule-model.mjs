#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { resolve, relative } from "node:path";

const root = resolve(process.argv[2] ?? process.cwd());
const roots = ["src-tauri/crates", "src-tauri/src", "test-support"];
const forbidden = [
  /\bProtocolDocument(?:Rule\w*|Predicate|Operation)\b/u,
  /\bProtocolRuleStage\b/u,
  /\bprotocol_document_rule\b/u,
  /\bRuleEngine\b/u,
  /\bRuleRepositoryPort\b/u,
  /\bRuleDraft\b/u,
  /\bConditionTree\b/u,
  /\bRuleStage::TlsHandshake\b/u,
  /\bRejectTlsHandshake\b/u,
  /\bpub\s+(?:struct|enum|type)\s+Rule\b/u,
  /\bfrom_http_conditions\b/u,
  /\bhttp_runtime_rules\s*\(/u,
  /\breplace_http_runtime_rules\s*\(/u,
];
const forbiddenPaths = [
  "src-tauri/crates/domain/src/rule/engine.rs",
  "src-tauri/crates/domain/src/workspace/runtime_projection.rs",
];

function rustFiles(directory) {
  const absolute = resolve(root, directory);
  return readdirSync(absolute).flatMap((entry) => {
    const path = resolve(absolute, entry);
    if (statSync(path).isDirectory()) return rustFiles(relative(root, path));
    return path.endsWith(".rs") ? [path] : [];
  });
}

const failures = [];
for (const path of forbiddenPaths) {
  if (existsSync(resolve(root, path))) {
    failures.push(`${path}: second rule model owner must be deleted`);
  }
}
for (const path of roots.flatMap(rustFiles)) {
  const source = readFileSync(path, "utf8");
  for (const pattern of forbidden) {
    if (pattern.test(source)) {
      failures.push(`${relative(root, path)}: legacy rule model remains (${pattern.source})`);
    }
  }
}

if (failures.length > 0) {
  process.stderr.write(`${failures.join("\n")}\n`);
  process.exit(1);
}

process.stdout.write("unified rule model: PASS\n");
