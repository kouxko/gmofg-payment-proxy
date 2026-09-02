#!/usr/bin/env -S deno run -A

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(process.argv[2] ?? process.cwd());
const owners = {
  actor: "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor.rs",
  actorEvaluation: "src-tauri/crates/infrastructure/src/adapters/pipeline/rule_runtime/actor/evaluation.rs",
  http: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/http_protocol_pipeline.rs",
  socket: "src-tauri/crates/infrastructure/src/adapters/listener_runtime/external_relay/contract.rs",
  repository: "src-tauri/crates/infrastructure/src/adapters/rules.rs",
};
const source = Object.fromEntries(
  Object.entries(owners).map(([name, path]) => [name, readFileSync(resolve(root, path), "utf8")]),
);
const failures = [];
if (/\bfallback_schema\b/u.test(source.http)) failures.push("HTTP runtime must preserve absent schema as None");
if (/validated Socket Manifest requires (?:upstream|downstream) schema/u.test(source.socket)) {
  failures.push("Socket runtime must preserve absent schema as None");
}
const runtimeSnapshot = source.repository.match(
  /pub async fn runtime_snapshot[\s\S]*?pub async fn commit_runtime_deltas/u,
)?.[0] ?? "";
if (!/workspace\.rule_definitions\.clone\(\)/u.test(runtimeSnapshot)
  || /\.runtime_rules\(\)|\.http_runtime_rules\(\)/u.test(runtimeSnapshot)) {
  failures.push("production runtime snapshot must own RuleDefinition without a legacy Rule projection");
}
if (/\bRuleEngine\b|\.runtime_rules\(\)|\.http_runtime_rules\(\)/u.test(
  source.actor + source.actorEvaluation,
)) {
  failures.push("actor must not split message rules between RuleEngine and a Document gate");
}
if (failures.length) {
  process.stderr.write(`${failures.join("\n")}\n`);
  process.exit(1);
}
process.stdout.write("unified exchange runtime: PASS\n");
