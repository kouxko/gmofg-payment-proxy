import { describe, expect, it } from "vitest";
import {
  isProtocolPackageGroupList,
  packageStatus,
  protocolPackageDetailError,
  sortPackageVersions,
  validationText,
  packageSourceText,
} from "./protocol-package-model";
import { detail, group, version } from "./protocol-packages-test-support";

describe("protocol package presentation model", () => {
  it("reverses the authoritative Rust SemVer order without reparsing numbers", () => {
    const sorted = sortPackageVersions([
      version("1.0.0-alpha.2"),
      version("1.0.0-alpha.10"),
      version("1.0.0+build.1"),
      version("1.0.0+build.2"),
      version("18446744073709551614.0.0"),
      version("18446744073709551615.0.0"),
    ]);

    expect(sorted.map((item) => item.package.version)).toEqual([
      "18446744073709551615.0.0",
      "18446744073709551614.0.0",
      "1.0.0+build.2",
      "1.0.0+build.1",
      "1.0.0-alpha.10",
      "1.0.0-alpha.2",
    ]);
  });

  it("derives enabled, disabled and invalid group states", () => {
    expect(packageStatus([version("1.0.0")])).toMatchObject({ label: "已停用", color: "default" });
    expect(packageStatus([version("2.0.0")])).toMatchObject({ label: "已启用", color: "success" });
    expect(packageStatus([version("1.0.0"), version("2.0.0")])).toMatchObject({
      label: "部分启用 1/2",
      color: "warning",
    });
    expect(packageStatus([
      version("3.0.0", { validation: { state: "invalid", code: "BROKEN" } }),
    ])).toMatchObject({ label: "校验失败", color: "danger", invalidCount: 1 });
  });

  it("formats validation errors with and without a backend code", () => {
    expect(validationText({ state: "invalid", code: "SCHEMA_INVALID" })).toBe("校验失败：SCHEMA_INVALID");
    expect(validationText({ state: "invalid", code: "" })).toBe("校验失败：未知错误");
  });

  it("uses the closed source union for managed and remote status text", () => {
    expect(packageSourceText(version("1.0.0", {
      package_source: { type: "managed", online: true },
    }))).toBe("本地管理 · 运行中");
    expect(packageSourceText(version("1.0.0", {
      package_source: { type: "external", online: true },
    }))).toBe("远端调试 · 在线");
    expect(packageSourceText(version("1.0.0", {
      package_source: { type: "external", online: false },
    }))).toBe("远端调试 · 离线");
    const malformed: unknown = [{
      ...group(),
      versions: [{
        ...version("1.0.0"),
        package_source: { type: "external", online: false },
        legacy_online: false,
      }],
    }];
    expect(isProtocolPackageGroupList(malformed)).toBe(false);
  });

  it("rejects cross-kind versions and unknown list fields", () => {
    expect(isProtocolPackageGroupList([group({
      kind: "http",
      versions: [version("1.0.0", { kind: "socket" })],
    })])).toBe(false);
    expect(isProtocolPackageGroupList([{
      ...group(),
      legacy_schema: "schema.json",
    }])).toBe(false);
    expect(isProtocolPackageGroupList([group({
      kind: "http",
      versions: [version("1.0.0", { kind: "http" })],
    })])).toBe(true);
  });

  it("validates Frame capability against the detail package kind", () => {
    const expected = { id: "iso-8583", version: "2.0.0" };
    const http = detail(version("2.0.0", { kind: "http" }), {
      kind: "http",
      capabilities: {
        upstream: { frame: false, decode: true, encode: true },
        downstream: { frame: false, decode: true, encode: true },
        display: true,
      },
    });
    expect(protocolPackageDetailError(http, expected)).toBeUndefined();
    expect(protocolPackageDetailError({
      ...http,
      version: { ...http.version, kind: "socket" },
    }, expected)).toBe("协议包详情数据不完整。");
    expect(protocolPackageDetailError({
      ...http,
      capabilities: {
        ...http.capabilities,
        upstream: { ...http.capabilities.upstream, frame: true },
      },
    }, expected)).toBe("协议包详情数据不完整。");

    const socket = detail();
    expect(protocolPackageDetailError({
      ...socket,
      capabilities: {
        ...socket.capabilities,
        downstream: { ...socket.capabilities.downstream, frame: false },
      },
    }, expected)).toBe("协议包详情数据不完整。");
  });

  it("rejects the removed local_process compatibility field", () => {
    const expected = { id: "iso-8583", version: "2.0.0" };
    const current = detail();
    const legacy = {
      ...current,
      external: { ...current.external!, local_process: false },
    };

    expect(protocolPackageDetailError(current, expected)).toBeUndefined();
    expect(protocolPackageDetailError(legacy, expected)).toBe("协议包详情数据不完整。");
  });

  it("rejects malformed and extended recursive Schema nodes", () => {
    const expected = { id: "iso-8583", version: "2.0.0" };
    const base = detail();
    for (const root of [
      { type: "array" },
      { type: "float" },
      { type: "string", legacy: true },
    ]) {
      expect(protocolPackageDetailError({
        ...base,
        upstream_schema: { root: root as never },
      }, expected)).toBe("协议包详情数据不完整。");
    }
  });
});
