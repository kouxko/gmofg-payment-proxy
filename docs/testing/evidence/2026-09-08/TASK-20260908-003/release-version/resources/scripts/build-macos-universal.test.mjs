import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const buildScriptPath = path.join(
  import.meta.dirname,
  "build-macos-universal.mjs",
);

test("macOS DMG packaging uses the stable hdiutil srcfolder contract", async () => {
  const source = await readFile(buildScriptPath, "utf8");

  assert.match(source, /run\("hdiutil", \[/u);
  assert.match(source, /"create",/u);
  assert.match(source, /"-volname",\s*"Intercept Proxy",/u);
  assert.match(source, /"-srcfolder",\s*dmgSource,/u);
  assert.match(source, /"-format",\s*"UDZO",/u);
  assert.doesNotMatch(source, /run\("diskutil",/u);
  assert.doesNotMatch(source, /--volumeName/u);
});
