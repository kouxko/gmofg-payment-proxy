import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const repositoryRoot = path.resolve(import.meta.dirname, "..");
const maximumLines = Number.parseInt(process.env.SOURCE_LINE_LIMIT ?? "500", 10);

if (!Number.isSafeInteger(maximumLines) || maximumLines < 1) {
  throw new Error("SOURCE_LINE_LIMIT 必须是大于 0 的整数。");
}

const sourceRoots = [
  "src",
  "src-tauri/src",
  "src-tauri/crates",
  "android-companion/app/src/main",
  "android-companion/app/src/test",
  "android-companion/app/src/androidTest",
];
const sourceExtensions = new Set([".rs", ".ts", ".tsx", ".kt", ".java"]);

function shouldInspect(relativePath) {
  const normalized = relativePath.split(path.sep).join("/");
  const basename = path.basename(normalized);
  return (
    sourceExtensions.has(path.extname(basename)) &&
    !normalized.includes("/target/") &&
    !normalized.includes("/generated/")
  );
}

async function collectFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const absolutePath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectFiles(absolutePath)));
    } else if (entry.isFile()) {
      files.push(absolutePath);
    }
  }
  return files;
}

const oversized = [];
for (const root of sourceRoots) {
  const absoluteRoot = path.join(repositoryRoot, root);
  for (const absolutePath of await collectFiles(absoluteRoot)) {
    const relativePath = path.relative(repositoryRoot, absolutePath);
    if (!shouldInspect(relativePath)) continue;
    const contents = await readFile(absolutePath, "utf8");
    const lineCount = contents.length === 0 ? 0 : contents.split(/\r?\n/u).length;
    if (lineCount > maximumLines) oversized.push({ relativePath, lineCount });
  }
}

if (oversized.length > 0) {
  oversized.sort((left, right) => right.lineCount - left.lineCount);
  console.error(`手写源码文件不得超过 ${maximumLines} 行：`);
  for (const file of oversized) console.error(`- ${file.relativePath}: ${file.lineCount} 行`);
  process.exitCode = 1;
} else {
  console.log(`手写源码文件行数门禁通过（上限 ${maximumLines} 行）。`);
}
