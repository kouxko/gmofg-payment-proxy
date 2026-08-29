import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const forbiddenContracts = [
  "ClearDocument",
  "clear_document",
  "字段值槽",
  "Schema 身份和结构",
  "MAX_PROTOCOL_RULE_INT_TEXT_BYTES",
  "Blob Hex",
  "整数文本不能超过",
];

export function findPhase3LegacyContracts(sources) {
  const failures = [];
  for (const [file, source] of Object.entries(sources)) {
    for (const contract of forbiddenContracts) {
      if (source.includes(contract)) failures.push(`${file}: forbidden ${contract}`);
    }
  }
  return failures;
}

async function rustSources(directory, relative = "") {
  const sources = {};
  for (const entry of await readdir(path.join(directory, relative), { withFileTypes: true })) {
    if (entry.isDirectory() && entry.name === "target") continue;
    const child = path.join(relative, entry.name);
    if (entry.isDirectory()) Object.assign(sources, await rustSources(directory, child));
    if (entry.isFile() && entry.name.endsWith(".rs")) {
      const file = path.join("src-tauri", child);
      sources[file] = await readFile(path.join(directory, child), "utf8");
    }
  }
  return sources;
}

async function main() {
  const sources = await rustSources(path.join(repositoryRoot, "src-tauri"));
  sources["src/generated/rust-types.ts"] = await readFile(
    path.join(repositoryRoot, "src/generated/rust-types.ts"),
    "utf8",
  );
  const failures = findPhase3LegacyContracts(sources);
  if (failures.length > 0) {
    for (const failure of failures) process.stderr.write(`FAIL ${failure}\n`);
    process.exitCode = 1;
    return;
  }
  process.stdout.write("PASS Phase 3 legacy field-slot, ClearDocument and scalar-text contracts are absent\n");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  await main();
}
