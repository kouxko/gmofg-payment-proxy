import { describe, expect, it } from "vitest";
import {
  builtInRestoreResultError,
  isProtocolPackageGroupList,
  packageStatus,
  protocolPackageDetailError,
  sortPackageVersions,
  validationText,
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

  it("accepts only the protected built-in exact identity as a restore result", () => {
    expect(builtInRestoreResultError({
      outcome: "installed",
      version: version("1.0.0", {
        package: { id: "iso8583-ascii-standard", version: "1.0.0" },
        built_in: true,
        enabled: true,
      }),
      kind: "socket",
      capabilities: {
        upstream: { frame: true, decode: true, encode: true },
        downstream: { frame: true, decode: true, encode: true },
        display: true,
      },
      upstream_schema: { id: "iso8583-request", version: 1, title: "ISO Request", fields: [{ name: "mti", label: "MTI", type: "string" }] },
      downstream_schema: { id: "iso8583-response", version: 1, title: "ISO Response", fields: [{ name: "code", label: "Code", type: "string" }] },
    })).toBeUndefined();
    expect(builtInRestoreResultError({
      outcome: "installed",
      version: version("1.0.0", { built_in: true }),
    })).toBe("内置示例恢复结果不完整，请刷新列表后重试。");
    expect(builtInRestoreResultError({
      outcome: "installed",
      version: version("1.0.0", {
        package: { id: "iso8583-ascii-standard", version: "1.0.0" },
        built_in: false,
      }),
    })).toBe("内置示例恢复结果不完整，请刷新列表后重试。");
  });

  it("rejects cross-kind versions and unknown list fields", () => {
    expect(isProtocolPackageGroupList([group({
      kind: "http",
      versions: [version("1.0.0", { kind: "socket" })],
    })])).toBe(false);
    expect(isProtocolPackageGroupList([{
      ...group(),
      legacy_schema: "document.toml",
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

  it("rejects empty, duplicate, and extended Schema fields", () => {
    const expected = { id: "iso-8583", version: "2.0.0" };
    const base = detail();
    for (const fields of [
      [],
      [{ name: "mti", label: "MTI", type: "string" }, { name: "mti", label: "Again", type: "int" }],
      [{ name: "mti", label: "MTI", type: "string", legacy: true }],
    ]) {
      expect(protocolPackageDetailError({
        ...base,
        upstream_schema: { ...base.upstream_schema, fields: fields as never },
      }, expected)).toBe("协议包详情数据不完整。");
    }
  });
});
