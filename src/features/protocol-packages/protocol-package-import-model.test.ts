import { describe, expect, it } from "vitest";
import {
  importResultError,
  isImportPreview,
  outcomeText,
  presentImportError,
} from "./protocol-package-import-model";
import { importPreview, importResult } from "./protocol-packages-test-support";

describe("protocol package import boundary model", () => {
  it("accepts only complete no-source previews", () => {
    expect(isImportPreview(importPreview())).toBe(true);
    expect(isImportPreview(null)).toBe(false);
    expect(isImportPreview(importPreview({ token: "" }))).toBe(false);
    expect(isImportPreview(importPreview({ disposition: "identity_conflict", token: null }))).toBe(true);
    expect(isImportPreview(importPreview({ disposition: "identity_conflict" }))).toBe(false);
    expect(isImportPreview(importPreview({ package: { id: "", version: "3.0.0" } }))).toBe(false);
    expect(isImportPreview(importPreview({ capabilities: undefined as never }))).toBe(false);
    expect(isImportPreview(importPreview({ capabilities: {
      ...importPreview().capabilities,
      display: false,
    } }))).toBe(false);
    expect(isImportPreview(importPreview({ capabilities: {
      ...importPreview().capabilities,
      downstream: { ...importPreview().capabilities.downstream, encode: false },
    } }))).toBe(false);
    expect(isImportPreview(importPreview({ upstream_schema: { root: { type: "array", items: {} as never } } }))).toBe(false);
    expect(isImportPreview(importPreview({ upstream_schema: { root: { type: "array" } as never } }))).toBe(false);
    expect(isImportPreview(importPreview({ upstream_schema: {
      root: { type: "object", properties: { mti: { type: "float" } as never } },
    } }))).toBe(false);
    expect(isImportPreview({ ...importPreview(), schema: importPreview().upstream_schema })).toBe(false);
    expect(isImportPreview({ ...importPreview(), content_types: ["application/json"] })).toBe(false);
  });

  it("rejects a malformed or identity-mismatched commit result", () => {
    const preview = importPreview();
    expect(importResultError(importResult(), preview)).toBeUndefined();
    expect(importResultError(null, preview)).toMatch(/不一致/);
    expect(importResultError({ ...importResult(), outcome: "unknown" }, preview)).toMatch(/不一致/);
    expect(importResultError({
      ...importResult(),
      version: { ...importResult().version, package: { id: "tlv", version: "3.0.0" } },
    }, preview)).toMatch(/不一致/);
    expect(importResultError({ ...importResult(), kind: "http" }, preview)).toMatch(/不一致/);
  });

  it("preserves stable backend errors and deduplicates position details", () => {
    expect(presentImportError({
      code: "MANIFEST_INVALID",
      message: "Manifest 无效",
      field_errors: { manifest: ["manifest.toml:2:3", "manifest.toml:2:3", ""] },
      diagnostic: {
        file: "manifest.toml",
        field: "hooks.upstream.receive.frame",
        line: 2,
        column: 3,
        entry: "frame",
      },
    })).toEqual({
      code: "MANIFEST_INVALID",
      message: "Manifest 无效",
      details: [
        "manifest.toml:2:3",
        "字段：hooks.upstream.receive.frame",
        "入口：frame",
      ],
    });
    expect(presentImportError(new Error("browser detail"))).toEqual({
      message: "无法连接应用核心，请确认桌面应用已完成初始化。",
      details: [],
    });
  });

  it("distinguishes installed and reused outcomes", () => {
    expect(outcomeText("installed")).toBe("协议包安装成功。");
    expect(outcomeText("reused")).toMatch(/复用精确版本/);
  });
});
