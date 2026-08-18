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
  {
    package: "intercept-proxy-domain",
    required: true,
    filePolicies: [
      {
        suffix: "crates/domain/src/protocol_document_rule.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "crates/domain/src/protocol_document_rule/validation.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "crates/domain/src/protocol_package/document.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "crates/domain/src/protocol_package/identity.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "crates/domain/src/protocol_package/schema.rs",
        thresholds: { functions: 100, lines: 100, regions: 100 },
      },
    ],
  },
  {
    package: "intercept-proxy-application",
    required: true,
    filePolicies: [
      {
        suffix: "crates/application/src/facade/android/runtime_owner.rs",
        thresholds: { functions: 100, lines: 100, regions: 100 },
      },
      {
        suffix: "crates/application/src/facade/protocol_rule_values.rs",
        thresholds: { functions: 100, lines: 100, regions: 100 },
      },
      {
        suffix: "crates/application/src/facade/protocol_rules.rs",
        thresholds: { lines: 90 },
      },
    ],
  },
  {
    package: "intercept-proxy-infrastructure",
    required: true,
    filePolicies: [
      { suffix: "adapters/android_adb/owner.rs", thresholds: { lines: 90 } },
      { suffix: "adapters/android_adb/reverse.rs", thresholds: { lines: 90 } },
      { suffix: "adapters/android_adb/reverse/preparation.rs", thresholds: { lines: 90 } },
      { suffix: "adapters/android_adb/runtime.rs", thresholds: { lines: 90 } },
      { suffix: "sqlite/android_runtime_owner.rs", thresholds: { lines: 90 } },
      {
        suffix: "adapters/listener_runtime/document_rules.rs",
        thresholds: { lines: 90 },
      },
      {
        suffix: "adapters/listener_runtime/http_protocol_pipeline/failure.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "adapters/listener_runtime/http_protocol_pipeline/programs.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "adapters/listener_runtime/local_responder/failure.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "adapters/listener_runtime/scripted_relay/failure.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "adapters/listener_runtime/socket_capture_publisher.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
    ],
  },
  {
    package: "intercept-proxy",
    required: true,
    filePolicies: [
      {
        suffix: "src-tauri/src/mcp/catalog.rs",
        thresholds: { functions: 95, lines: 95, regions: 95 },
      },
      {
        suffix: "src-tauri/src/mcp/query.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
      {
        suffix: "src-tauri/src/mcp/resources.rs",
        thresholds: { functions: 90, lines: 90, regions: 90 },
      },
    ],
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

function metricsFromSummary(summary) {
  return Object.fromEntries(
    Object.entries(summary).map(([name, value]) => [name, value.percent]),
  );
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
  const data = report?.data?.[0];
  const totals = data?.totals;
  if (!totals) {
    throw new Error(`${policy.package}: cargo-llvm-cov report did not contain totals`);
  }
  const metrics = metricsFromSummary(totals);
  if (policy.thresholds) assertCoverage(policy.package, metrics, policy.thresholds);
  const files = (policy.filePolicies ?? []).map((filePolicy) => {
    const file = data.files?.find((candidate) => candidate.filename.endsWith(filePolicy.suffix));
    if (!file) throw new Error(`${policy.package}: coverage file missing: ${filePolicy.suffix}`);
    const fileMetrics = metricsFromSummary(file.summary);
    assertCoverage(`${policy.package}:${filePolicy.suffix}`, fileMetrics, filePolicy.thresholds);
    return { ...filePolicy, metrics: fileMetrics };
  });
  return { package: policy.package, thresholds: policy.thresholds, metrics, files };
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
  for (const file of summary.files) {
    process.stdout.write(
      `${summary.package}:${file.suffix}: lines ${file.metrics.lines.toFixed(2)}%\n`,
    );
  }
}

const outputPath = resolve(repositoryRoot, "coverage/rust-summary.json");
mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify({ generatedBy: "cargo-llvm-cov 0.8.7", summaries }, null, 2)}\n`);
process.stdout.write(`Rust coverage summary written to ${outputPath}\n`);
