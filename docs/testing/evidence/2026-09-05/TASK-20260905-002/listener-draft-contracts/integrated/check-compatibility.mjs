import ts from "typescript";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
const binding = "src/generated/rust-types.ts";
const base = "c7a70d3ef63719ae3bb7f9b3e6dca8db995c6525";
function declarations(text) {
 const source = ts.createSourceFile(binding, text, ts.ScriptTarget.Latest, true);
 for (const statement of source.statements) {
  if (!ts.isVariableStatement(statement)) continue;
  for (const declaration of statement.declarationList.declarations) {
   if (declaration.name.getText(source) !== "commands") continue;
   if (!ts.isObjectLiteralExpression(declaration.initializer)) throw new Error("commands is not an object");
   return new Map(declaration.initializer.properties.map(p => [p.name.getText(source), p.getText(source)]));
  }
 }
 throw new Error("commands declaration missing");
}
const old = declarations(execFileSync("git", ["show", `${base}:${binding}`], {encoding:"utf8"}));
const current = declarations(readFileSync(binding, "utf8"));
const added = [...current.keys()].filter(k => !old.has(k)).sort();
const removed = [...old.keys()].filter(k => !current.has(k));
const changed = [...old.keys()].filter(k => current.has(k) && current.get(k) !== old.get(k));
const protectedPaths = ["src-tauri/crates/domain", "src/features/shared/protocol-safe-display.tsx", "src/features/shared/protocol-safe-display.test.tsx", "src-tauri/crates/package-runtime", "src-tauri/crates/infrastructure/src/sqlite"];
execFileSync("git", ["diff", "--exit-code", base, "--", ...protectedPaths]);
const expectedAdded = ["listenerDraftCopy", "listenerDraftTransition", "listenerDraftValidate"];
const pass = !removed.length && !changed.length && JSON.stringify(added) === JSON.stringify(expectedAdded);
console.log(JSON.stringify({result:pass ? "PASS" : "FAIL",comparison_base:base,legacy_command_count:old.size,added_commands:added,removed_commands:removed,changed_existing_command_declarations:changed,protected_paths_unchanged:protectedPaths,method:"TypeScript AST exact command declarations and targeted git diff; not a runtime acceptance claim"},null,2));
process.exitCode = pass ? 0 : 1;
