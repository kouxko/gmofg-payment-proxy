import ts from "typescript";
import { ESLint } from "eslint";
import path from "node:path";
const root = process.cwd();
const raw = ts.readConfigFile(path.join(root, "tsconfig.json"), ts.sys.readFile);
if (raw.error) throw new Error(ts.flattenDiagnosticMessageText(raw.error.messageText, "\n"));
const parsed = ts.parseJsonConfigFileContent(raw.config, ts.sys, root);
if (parsed.errors.length) throw new Error(JSON.stringify(parsed.errors));
const files = new Set(parsed.fileNames.map(p => path.relative(root, p)));
const eslint = new ESLint({ cwd: root });
const cases = [
 ["docs/testing/evidence/2026-09-05/TASK-20260905-001/integrated-verification/resources/src/features/listeners/listener-workspace-draft.ts", false, true],
 ["src/features/listeners/listener-workspace-draft.ts", true, false],
 ["src/features/listeners/listener-workspace-draft.test.ts", true, false],
 ["vitest.config.ts", true, false],
];
let failed = false;
for (const [file, expectedIncluded, expectedIgnored] of cases) {
 const actual = { included: files.has(file), ignored: await eslint.isPathIgnored(file) };
 const pass = actual.included === expectedIncluded && actual.ignored === expectedIgnored;
 console.log(JSON.stringify({file, expected:{included:expectedIncluded,ignored:expectedIgnored},actual,pass}));
 failed ||= !pass;
}
process.exitCode = failed ? 1 : 0;
