import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const repo = resolve(import.meta.dirname, "..");
const checker = resolve(import.meta.dirname, "check-task-20260829-002-phase16-docs.mjs");
const files = [
  "docs/architecture/decisions/ADR-009-nested-document-javascript-package-runtime.md",
  "docs/README.md",
  "docs/architecture/rules-and-protocol-packages.md",
  "docs/architecture/data-flow.md",
  "docs/architecture/modules.md",
  "docs/user-operation-guide.md",
  "docs/onboarding-guide.md",
  "templates/socket-protocol/AUTHORING.md",
  "templates/socket-protocol/API.md",
  "docs/mcp/external-package-integration-guide.md",
  "docs/mcp/diagnostic-architecture.md",
  "docs/mcp/tool-reference.md",
  "docs/mcp/validation-playbook.md",
  "docs/testing/release-validation-matrix.md",
  "docs/testing/evidence/TEMPLATE.md",
  "docs/architecture/exchange-pipeline-template/README.md",
  "src-tauri/src/mcp/resources.rs",
  "src-tauri/src/mcp/catalog.rs",
];

function run(root) {
  return spawnSync(process.execPath, [checker, root], { encoding: "utf8" });
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "phase16-docs-"));
  for (const relative of files) {
    const source = join(repo, relative);
    const target = join(root, relative);
    cpSync(source, target, { recursive: true });
  }
  return root;
}

test("current documentation satisfies the final Phase1-15 contract", () => {
  const result = run(repo);
  assert.equal(result.status, 0, result.stderr);
});

test("checker rejects removal of an authoritative current contract", () => {
  const root = fixture();
  try {
    const path = join(root, "docs/architecture/rules-and-protocol-packages.md");
    writeFileSync(path, readFileSync(path, "utf8").replaceAll("只 Encode 一次", "REMOVED"));
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects legacy active-package guidance", () => {
  const root = fixture();
  try {
    const path = join(root, "docs/user-operation-guide.md");
    writeFileSync(path, `${readFileSync(path, "utf8")}\nmanifest.toml\n`);
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects collapsing local Boa bytes into the public JSON-RPC wire", () => {
  const root = fixture();
  try {
    const path = join(root, "templates/socket-protocol/API.md");
    writeFileSync(path, readFileSync(path, "utf8").replaceAll("canonical padded Base64", "REMOVED_WIRE"));
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects loss of bounded typed process evidence", () => {
  const root = fixture();
  try {
    const path = join(root, "docs/testing/evidence/TEMPLATE.md");
    writeFileSync(path, readFileSync(path, "utf8").replaceAll("changes_truncated", "REMOVED_LOSS"));
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects reviewed documentation regressions", () => {
  for (const [relative, token] of [
    ["docs/architecture/rules-and-protocol-packages.md", "原始快照匹配"],
    ["docs/mcp/external-package-integration-guide.md", "RPC timeout 或并发额度"],
    ["docs/mcp/external-package-integration-guide.md", "不会隐式启用"],
    ["docs/user-operation-guide.md", "loopback WebSocket `/packages`"],
    ["docs/testing/release-validation-matrix.md", "兼容模式使用用户数据库"],
    ["docs/testing/release-validation-matrix.md", "上下行 hook、断线、重连、超时、限额"],
    ["docs/testing/release-validation-matrix.md", "调用顺序和超时预算"],
    ["docs/testing/release-validation-matrix.md", "详情只显示固定的 Display HTML"],
    ["docs/architecture/exchange-pipeline-template/README.md", "权威设计模板"],
  ]) {
    const root = fixture();
    try {
      const path = join(root, relative);
      writeFileSync(path, `${readFileSync(path, "utf8")}\n${token}\n`);
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects loss of public package registration and error wire", () => {
  for (const token of ["without an `id`", "Proxy sends no response", "error.data.code", "canonical padded Base64"]) {
    const root = fixture();
    try {
      const path = join(root, "templates/socket-protocol/API.md");
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_API1_WIRE"));
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects loss of ADR-009 discoverability and current authority", () => {
  for (const token of [
    "ADR-002：HTTP 协议包（已被 ADR-009 替代）",
    "ADR-007：Exchange/Pipeline 运行边界（已被 ADR-009 替代）",
    "ADR-009：递归 Document、统一规则与 JavaScript 协议包运行时",
    "当前实现以 ADR-009、当前架构文档和源码为准",
    "不再是 current authority",
  ]) {
    const root = fixture();
    try {
      const path = join(root, "docs/README.md");
      writeFileSync(path, readFileSync(path, "utf8").replaceAll(token, "REMOVED_ADR_AUTHORITY"));
      assert.notEqual(run(root).status, 0, token);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("checker rejects overstating the current Boa host as a general sandbox", () => {
  const root = fixture();
  try {
    const path = join(root, "templates/socket-protocol/AUTHORING.md");
    writeFileSync(path, `${readFileSync(path, "utf8")}\n不向脚本暴露网络、文件、进程\n`);
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checker rejects drifting the current MCP read-tool count from 36 to 37", () => {
  const root = fixture();
  try {
    const path = join(root, "docs/user-operation-guide.md");
    writeFileSync(path, readFileSync(path, "utf8").replaceAll("36 个", "37 个"));
    assert.notEqual(run(root).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
