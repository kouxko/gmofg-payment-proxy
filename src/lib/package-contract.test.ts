import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

import {
  isFrameResult,
  isPackageDocument,
  isPackageManifest,
  isPackageRegisterNotification,
  isPackageRpcFailure,
  isPackageRpcRequest,
  isPackageRpcSuccess,
} from "./package-contract";

const fixtureRoot = path.join(process.cwd(), "test-support/fixtures/task-20260829-002/phase-4/package-contract");
const fixture = (name: string): unknown => JSON.parse(readFileSync(path.join(fixtureRoot, name), "utf8"));

describe("package contract unknown-boundary guards", () => {
  it("accepts shared Rust manifest and registration fixtures", () => {
    expect(isPackageManifest(fixture("http-manifest.json"))).toBe(true);
    expect(isPackageManifest(fixture("socket-manifest.json"))).toBe(true);
    expect(isPackageRegisterNotification(fixture("register-notification.json"))).toBe(true);
  });

  it("rejects unknown manifest fields and missing Socket schemas", () => {
    expect(isPackageManifest({ ...(fixture("http-manifest.json") as object), hooks: {} })).toBe(false);
    const socket = structuredClone(fixture("socket-manifest.json")) as Record<string, unknown>;
    (socket.document as Record<string, unknown>).upstream = {};
    expect(isPackageManifest(socket)).toBe(false);
  });

  it("accepts exact fixed RPC requests and rejects shape reinterpretation", () => {
    const rpc = fixture("rpc-examples.json") as { requests: unknown[] };
    expect(rpc.requests.every(isPackageRpcRequest)).toBe(true);
    expect(isPackageRpcRequest({ jsonrpc: "2.0", id: 1, method: "hooks.upstream.frame", params: { buffer: "AA==" } })).toBe(false);
    expect(isPackageRpcRequest({ jsonrpc: "2.0", id: "x", method: "hooks.upstream.frame", params: { buffer: "AA" } })).toBe(false);
  });

  it("enforces the closed frame result union", () => {
    expect(isFrameResult({ status: "need_more", requiredBytes: 3 })).toBe(true);
    expect(isFrameResult({ status: "complete", consumedBytes: 1 })).toBe(true);
    expect(isFrameResult({ status: "reject", reason: "bad" })).toBe(true);
    expect(isFrameResult({ status: "complete", consumedBytes: 0 })).toBe(false);
    expect(isFrameResult({ status: "reject", reason: "bad", consumedBytes: 1 })).toBe(false);
  });

  it("validates every canonical golden request, result and stable-code error", () => {
    const golden = fixture("golden.json") as Record<string, unknown>;
    expect(isPackageManifest(golden.manifest)).toBe(true);
    expect(isPackageRegisterNotification(golden.registration)).toBe(true);
    expect((golden.requests as unknown[]).every(isPackageRpcRequest)).toBe(true);
    const successes = golden.successes as Record<string, unknown>;
    expect((successes.frame as unknown[]).every((value) => isPackageRpcSuccess(value, isFrameResult))).toBe(true);
    expect(isPackageRpcSuccess(successes.decode, isPackageDocument)).toBe(true);
    expect(isPackageRpcSuccess(successes.encode, (value) => typeof value === "string")).toBe(true);
    expect(isPackageRpcSuccess(successes.display, (value) => typeof value === "string")).toBe(true);
    expect(isPackageRpcFailure(golden.failure)).toBe(true);
    const unknownCode = structuredClone(golden.failure) as Record<string, unknown>;
    ((unknownCode.error as Record<string, unknown>).data as Record<string, unknown>).code = "UNKNOWN_CODE";
    expect(isPackageRpcFailure(unknownCode)).toBe(false);
    for (const invalidDocument of [
      { amount: Number.MAX_SAFE_INTEGER + 1 },
      { nested: [true, { illegal: undefined }] },
    ]) {
      const invalidDecode = structuredClone(successes.decode) as Record<string, unknown>;
      invalidDecode.result = invalidDocument;
      expect(isPackageRpcSuccess(invalidDecode, isPackageDocument)).toBe(false);
    }
  });

  it("rejects Domain-invalid identity, metadata and nested Schema title values", () => {
    const valid = fixture("http-manifest.json") as Record<string, unknown>;
    for (const mutate of [
      (value: Record<string, unknown>) => { (value.package as Record<string, unknown>).id = "Invalid.ID"; },
      (value: Record<string, unknown>) => { (value.package as Record<string, unknown>).version = "01.0.0"; },
      (value: Record<string, unknown>) => { (value.package as Record<string, unknown>).name = "   "; },
      (value: Record<string, unknown>) => {
        (value.document as Record<string, unknown>).upstream = {
          schema: { type: "object", properties: { nested: { type: "string", title: " " } } },
        };
      },
    ]) {
      const invalid = structuredClone(valid);
      mutate(invalid);
      expect(isPackageManifest(invalid)).toBe(false);
    }
  });

  it("matches the shared Domain package ID corpus, including dotted IDs", () => {
    const valid = fixture("http-manifest.json") as Record<string, unknown>;
    const corpus = fixture("validation-corpus.json") as {
      id: Array<{ value: string; valid: boolean }>;
    };
    for (const testCase of corpus.id) {
      const manifest = structuredClone(valid);
      (manifest.package as Record<string, unknown>).id = testCase.value;
      expect(isPackageManifest(manifest), testCase.value).toBe(testCase.valid);
    }
  });
});
