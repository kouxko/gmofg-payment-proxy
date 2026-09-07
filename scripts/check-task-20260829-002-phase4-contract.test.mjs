import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { promisify } from "node:util";

import { checkPhase4, validateCanonicalGoldenSnapshot, validateManifestSnapshot } from "./check-task-20260829-002-phase4-contract.mjs";

const inventoryPath = path.join(import.meta.dirname, "../test-support/fixtures/task-20260829-002/phase-4/package-contract/inventory.json");
const inventory = async () => JSON.parse(await readFile(inventoryPath, "utf8"));
const execFileAsync = promisify(execFile);
const declaredTests = (value) => Object.fromEntries(
  Object.entries(value.required_test_targets).map(([target, names]) => [target, [...names]]),
);

test("Phase 4 checker accepts the complete current contract", async () => {
  assert.deepEqual(await checkPhase4(), []);
});

test("Phase 4 checker fails closed on zero required test names", async () => {
  const value = await inventory();
  value.required_test_targets.manifest_contract = [];
  assert((await checkPhase4({ inventory: value, discoveredTests: declaredTests(value) })).some((failure) => failure.includes("nonzero")));
});

test("Phase 4 checker uses Cargo discovery so a comment cannot fake a required test", async () => {
  const value = await inventory();
  const discoveredTests = declaredTests(value);
  discoveredTests.rpc_contract = discoveredTests.rpc_contract.filter((name) => name !== "package_register_is_an_id_less_one_way_notification");
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    return file.endsWith("rpc_contract.rs")
      ? `${source}\n// fn package_register_is_an_id_less_one_way_notification() {}\n`
      : source;
  };
  const failures = await checkPhase4({ inventory: value, discoveredTests, read });
  assert(failures.some((failure) => failure.includes("Cargo did not discover required test")));
});

test("Phase 4 checker fails closed on reverse dependency and MCP fixture drift", async () => {
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (file.endsWith("crates/domain/Cargo.toml")) return `${source}\nintercept-proxy-package-contract = { path = \"../package-contract\" }\n`;
    if (file.endsWith("schema.snapshot.json")) return source.replace('"package.register"', '"package.register.v2"');
    return source;
  };
  const value = await inventory();
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("must not depend back")));
  assert(failures.some((failure) => failure.includes("registration semantic contract drift")));
});

test("Phase 4 checker rejects a differently named Manifest-shaped second owner", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (file.endsWith("src-tauri/src/lib.rs")) {
      return `${source}\n#[derive(serde::Serialize)]\npub struct AlternateWire { api: u32, kind: String, package: String, document: String }\n`;
    }
    return source;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker rejects serde-renamed Manifest-shaped second owners", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}\n#[derive(serde::Serialize)]\npub struct AlternateWire {\n  api: u32,\n  kind: String,\n  #[serde(rename = "package")]\n  metadata: String,\n  #[serde(rename="document")]\n  payload: String,\n}\n`;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker rejects serde alias and deserialize-only Manifest wire owners", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}\n#[derive(serde::Deserialize)]\npub struct InputWire {\n  api: u32,\n  kind: String,\n  #[serde(alias = "package")]\n  metadata: String,\n  #[serde(rename(deserialize = "document"))]\n  payload: String,\n}\n`;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker rejects private and untagged-enum Manifest wire owners", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}
#[derive(serde::Serialize)]
struct PrivateManifestWire { api: u32, kind: String, package: String, document: String }

#[derive(serde::Serialize)]
#[serde(untagged)]
enum AlternateEnvelope {
  Manifest { api: u32, kind: String, package: String, document: String },
  Other(String),
}
`;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("PrivateManifestWire") && failure.includes("Manifest-shaped wire owner")));
  assert(failures.some((failure) => failure.includes("AlternateEnvelope::Manifest") && failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker resolves serde flatten and rejects an actual flattened Manifest owner", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}
#[derive(serde::Serialize)]
struct ManifestPayload {
  package: String,
  document: String,
}
#[derive(serde::Serialize)]
struct FlattenedWire {
  api: u32,
  kind: String,
  #[serde(flatten)]
  payload: ManifestPayload,
}
`;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("FlattenedWire") && failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker ignores harmless flatten, comments and non-Serde internal shapes", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}
#[derive(serde::Serialize)]
struct Filters { trace: bool, limit: usize }
#[derive(serde::Serialize)]
struct HarmlessRequest {
  path: String,
  #[serde(flatten)]
  filters: Filters,
}
// #[derive(serde::Serialize)]
// struct PhantomManifest { api: u32, kind: String, package: String, document: String }
const PHANTOM: &str = "struct StringManifest { api: u32, kind: String, package: String, document: String }";
struct InternalShape { api: u32, kind: String, package: String, document: String }
`;
  };
  assert.deepEqual(await checkPhase4({ read, discoveredTests: declaredTests(value) }), []);
});

test("Phase 4 checker rejects custom-key manual Serialize Manifest owners", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}
struct ManualOutput { metadata: String, payload: String }
impl serde::Serialize for ManualOutput {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let mut state = serializer.serialize_struct("ManualOutput", 4)?;
    state.serialize_field("api", &1)?;
    state.serialize_field("kind", "http")?;
    state.serialize_field("package", &self.metadata)?;
    state.serialize_field("document", &self.payload)?;
    state.end()
  }
}
`;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("ManualOutput") && failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker rejects custom-key manual Deserialize Visitor Manifest owners", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}
struct ManualInput { metadata: String, payload: String }
struct ManualInputVisitor;
impl<'de> serde::de::Visitor<'de> for ManualInputVisitor {
  fn visit_map<A>(self, mut map: A) -> Result<ManualInput, A::Error> {
    while let Some(key) = map.next_key::<String>()? {
      match key.as_str() {
        "api" => {},
        "kind" => {},
        "package" => {},
        "document" => {},
        _ => {},
      }
    }
    todo!()
  }
}
impl<'de> serde::Deserialize<'de> for ManualInput {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
    const FIELDS: &[&str] = &["api", "kind", "package", "document"];
    deserializer.deserialize_struct("ManualInput", FIELDS, ManualInputVisitor)
  }
}
`;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("ManualInput") && failure.includes("Manifest-shaped wire owner")));
});

test("Phase 4 checker accepts harmless manual Serde with Manifest-like declared fields", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("src-tauri/src/lib.rs")) return source;
    return `${source}
struct HarmlessManual { api: u32, kind: String, package: String, document: String }
impl serde::Serialize for HarmlessManual {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str("harmless")
  }
}
`;
  };
  assert.deepEqual(await checkPhase4({ read, discoveredTests: declaredTests(value) }), []);
});

test("Phase 4 checker rejects generated stale types and any exact generated drift", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    return file.endsWith("src/generated/rust-types.ts")
      ? `${source}\nexport type ExternalFrameResult = {};\n`
      : source;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("exact recorded Rust export")));
  assert(failures.some((failure) => failure.includes("forbidden stale type")));
});

test("generated ID and SemVer metadata execute identically to Domain constructors", async () => {
  const corpus = JSON.parse(await readFile(path.join(import.meta.dirname, "../test-support/fixtures/task-20260829-002/phase-4/package-contract/validation-corpus.json"), "utf8"));
  const generated = await readFile(path.join(import.meta.dirname, "../src/generated/rust-types.ts"), "utf8");
  const metadata = JSON.parse(generated.match(/export const PACKAGE_CONTRACT_VALIDATION = (\{.*\}) as const;/u)?.[1] ?? "");
  const { stdout } = await execFileAsync("cargo", [
    "run", "--quiet", "--manifest-path", "src-tauri/Cargo.toml", "-p",
    "intercept-proxy-package-contract", "--example", "validation-metadata",
  ], { cwd: path.join(import.meta.dirname, "..") });
  const rust = JSON.parse(stdout);
  const idPattern = new RegExp(metadata.packageIdPattern, "u");
  const versionPattern = new RegExp(metadata.packageVersionPattern, "u");
  for (const area of ["id", "version"]) {
    assert.equal(rust[area].length, corpus[area].length);
    for (let index = 0; index < corpus[area].length; index += 1) {
      const expected = corpus[area][index];
      const rustCase = rust[area][index];
      const generatedValid = area === "id"
        ? idPattern.test(expected.value) && expected.value.length <= metadata.packageIdMaxBytes
        : versionPattern.test(expected.value)
          && expected.value.length <= metadata.packageVersionMaxBytes
          && expected.value.split(/[+-]/u, 1)[0].split(".")
            .every((part) => BigInt(part) <= BigInt(metadata.packageVersionCoreNumericMax));
      assert.deepEqual(rustCase, { value: expected.value, valid: expected.valid });
      assert.equal(generatedValid, rustCase.valid, `${area}: ${expected.value}`);
    }
  }
});

test("Phase 4 checker rejects complete MCP snapshot drift", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    return file.endsWith("schema.snapshot.json") ? source.replace('"reject"', '"rejected"') : source;
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("exact recorded contract")));
});

test("Phase 4 checker rejects semantic MCP drift even when snapshot and evidence hashes are coherently updated", async () => {
  const value = await inventory();
  const snapshotPath = path.join(import.meta.dirname, "../src-tauri/src/mcp/tests/fixtures/package_contract_api1/schema.snapshot.json");
  const original = await readFile(snapshotPath, "utf8");
  const mutated = original.replace('"reject"', '"legacy_retry"');
  const hash = createHash("sha256").update(mutated).digest("hex");
  value.mcp_snapshot_sha256 = hash;
  const evidenceCopy = value.evidence_byte_copies.find((copy) => copy.source.endsWith("schema.snapshot.json"));
  evidenceCopy.sha256 = hash;
  const read = async (file, encoding) => {
    if (file.endsWith("schema.snapshot.json") || file.endsWith("resources/mcp-schema.snapshot.json")) {
      return encoding ? mutated : Buffer.from(mutated);
    }
    return readFile(file, encoding);
  };
  const failures = await checkPhase4({ inventory: value, read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("semantic contract drift")));
});

test("Phase 4 checker rejects broad and stale Phase 7 allowlist entries", async () => {
  const value = await inventory();
  value.phase7_legacy_wire_allowlist.push({
    file: "src-tauri/crates/domain/src/external_package",
    symbol: "ExternalAnything",
    reason: "broad path",
  });
  value.phase7_legacy_wire_allowlist.push({
    file: "src-tauri/crates/domain/src/external_package/runtime.rs",
    symbol: "ExternalNeverUsed",
    reason: "stale exact entry",
  });
  const failures = await checkPhase4({ inventory: value, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("exact .rs file")));
  assert(failures.some((failure) => failure.includes("stale or unused")));
});

test("Phase 4 checker forbids reallowing a migrated Phase 7 legacy wire", async () => {
  const value = await inventory();
  value.phase7_legacy_wire_allowlist = [{
    file: "src-tauri/crates/domain/src/lib.rs",
    symbol: "ExternalFrameRequest",
    reason: "attempt to restore a completed Phase 7 migration",
  }];
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    return file.endsWith("crates/domain/src/lib.rs")
      ? `${source}\npub struct ExternalFrameRequest;\n`
      : source;
  };
  const failures = await checkPhase4({ inventory: value, read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("must remain empty after Phase 7 migration")));
});

test("Phase 4 checker rejects a stale generated SHA in the active inventory", async () => {
  const value = await inventory();
  value.generated_sha256 = "0".repeat(64);
  const failures = await checkPhase4({ inventory: value, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("exact recorded Rust export")));
});

test("Phase 4 checker rejects evidence resources that are not exact SHA-256 copies", async () => {
  const value = await inventory();
  const read = async (file, encoding) => {
    const source = await readFile(file, encoding);
    if (!file.endsWith("phase4-package-contract/resources/golden.json")) return source;
    return Buffer.concat([Buffer.from(source), Buffer.from("\n")]);
  };
  const failures = await checkPhase4({ read, discoveredTests: declaredTests(value) });
  assert(failures.some((failure) => failure.includes("resource SHA/bytes differ")));
});

test("independent MCP snapshot accepts positive manifests and rejects contract mutations", async () => {
  const fixtureRoot = path.join(import.meta.dirname, "../test-support/fixtures/task-20260829-002/phase-4/package-contract");
  const snapshot = JSON.parse(await readFile(path.join(import.meta.dirname, "../src-tauri/src/mcp/tests/fixtures/package_contract_api1/schema.snapshot.json"), "utf8"));
  const http = JSON.parse(await readFile(path.join(fixtureRoot, "http-manifest.json"), "utf8"));
  const socket = JSON.parse(await readFile(path.join(fixtureRoot, "socket-manifest.json"), "utf8"));
  assert.equal(validateManifestSnapshot(http, snapshot), true);
  assert.equal(validateManifestSnapshot(socket, snapshot), true);
  assert.equal(validateManifestSnapshot({ ...http, hooks: {} }, snapshot), false);
  const missingSchema = structuredClone(socket);
  missingSchema.document.downstream = {};
  assert.equal(validateManifestSnapshot(missingSchema, snapshot), false);
  const invalidSchema = structuredClone(http);
  invalidSchema.document.upstream = { schema: { type: "null" } };
  assert.equal(validateManifestSnapshot(invalidSchema, snapshot), false);
});

test("independent MCP schema validates the complete canonical golden and rejects cross-contract mutations", async () => {
  const snapshot = JSON.parse(await readFile(path.join(import.meta.dirname, "../src-tauri/src/mcp/tests/fixtures/package_contract_api1/schema.snapshot.json"), "utf8"));
  const golden = JSON.parse(await readFile(path.join(import.meta.dirname, "../src-tauri/src/mcp/tests/fixtures/package_contract_api1/golden.json"), "utf8"));
  assert.equal(validateCanonicalGoldenSnapshot(golden, snapshot), true);
  const mutations = [
    (value) => { value.registration.id = "not-a-notification"; },
    (value) => { value.requests[0].params = { input: "AP8=" }; },
    (value) => { value.successes.frame[2].result.status = "legacy_retry"; },
    (value) => { value.successes.encode.result = { bytes: "AP8=" }; },
    (value) => { value.failure.error.data.code = "UNKNOWN_STABLE_CODE"; },
    (value) => { value.failure.error.retryable = true; },
  ];
  for (const mutate of mutations) {
    const invalid = structuredClone(golden);
    mutate(invalid);
    assert.equal(validateCanonicalGoldenSnapshot(invalid, snapshot), false);
  }
});
