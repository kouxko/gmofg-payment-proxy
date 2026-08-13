import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { assertCoverage } from "./coverage-policy.mjs";

const EXPECTED_CARGO_LLVM_COV_VERSION = "0.8.7";
const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(scriptDirectory, "..");
const manifestPath = resolve(repositoryRoot, "src-tauri/Cargo.toml");
const policies = [
  {
    package: "intercept-proxy-runtime",
    required: true,
    thresholds: { functions: 75, lines: 80, regions: 79 },
  },
  {
    package: "intercept-proxy-protocol-scripting",
    required: false,
    activationFile: resolve(repositoryRoot, "src-tauri/crates/protocol-scripting/Cargo.toml"),
    thresholds: { functions: 90, lines: 90, regions: 90 },
  },
];

function runCargo(arguments_, options = {}) {
  const result = spawnSync("cargo", arguments_, {
    cwd: repositoryRoot,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.error) {
    throw new Error(`unable to execute cargo: ${result.error.message}`);
  }
  if (result.status !== 0) {
    const detail = options.capture ? `\n${result.stderr || result.stdout}` : "";
    throw new Error(`cargo ${arguments_.join(" ")} failed with status ${result.status}${detail}`);
  }
  return result.stdout;
}

function verifyToolVersion() {
  const version = runCargo(["llvm-cov", "--version"], { capture: true }).trim();
  const expected = `cargo-llvm-cov ${EXPECTED_CARGO_LLVM_COV_VERSION}`;
  if (version !== expected) {
    throw new Error(
      `expected ${expected}, found ${version || "no version"}; install with ` +
        `cargo install cargo-llvm-cov --version ${EXPECTED_CARGO_LLVM_COV_VERSION} --locked`,
    );
  }
}

function isActive(policy) {
  if (policy.required) return true;
  try {
    readFileSync(policy.activationFile);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function collectPackageCoverage(policy) {
  const reportArguments = [
    "llvm-cov",
    "--manifest-path",
    manifestPath,
    "--package",
    policy.package,
  ];
  runCargo([...reportArguments, "--all-features", "--no-report", "--quiet"]);
  const json = runCargo(
    ["llvm-cov", "report", ...reportArguments.slice(1), "--summary-only", "--json"],
    { capture: true },
  );
  const report = JSON.parse(json);
  const totals = report?.data?.[0]?.totals;
  if (!totals) {
    throw new Error(`${policy.package}: cargo-llvm-cov report did not contain totals`);
  }
  const metrics = Object.fromEntries(
    Object.entries(totals).map(([name, value]) => [name, value.percent]),
  );
  assertCoverage(policy.package, metrics, policy.thresholds);
  return { package: policy.package, thresholds: policy.thresholds, metrics };
}

verifyToolVersion();
const summaries = [];
for (const policy of policies) {
  if (!isActive(policy)) {
    process.stdout.write(`${policy.package}: not present yet; 90% feature gate is armed\n`);
    continue;
  }
  const summary = collectPackageCoverage(policy);
  summaries.push(summary);
  process.stdout.write(
    `${summary.package}: functions ${summary.metrics.functions.toFixed(2)}%, ` +
      `lines ${summary.metrics.lines.toFixed(2)}%, regions ${summary.metrics.regions.toFixed(2)}%\n`,
  );
}

const outputPath = resolve(repositoryRoot, "coverage/rust-summary.json");
mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify({ generatedBy: "cargo-llvm-cov 0.8.7", summaries }, null, 2)}\n`);
process.stdout.write(`Rust coverage summary written to ${outputPath}\n`);
