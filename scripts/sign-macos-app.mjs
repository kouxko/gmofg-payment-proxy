import { readdir } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";

const directory = path.resolve(
  process.argv[2] ?? "src-tauri/target/universal-apple-darwin/release/bundle/macos",
);
const apps = (await readdir(directory)).filter((entry) => entry.endsWith(".app"));
if (apps.length !== 1) {
  throw new Error(`expected one app in ${directory}, found ${apps.length}`);
}
const app = path.join(directory, apps[0]);
const result = spawnSync("codesign", ["--force", "--deep", "--sign", "-", app], {
  stdio: "inherit",
});
if (result.status !== 0) process.exit(result.status ?? 1);
console.log(app);
