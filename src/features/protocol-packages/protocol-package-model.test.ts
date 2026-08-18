import { describe, expect, it } from "vitest";
import {
  builtInRestoreResultError,
  packageStatus,
  sortPackageVersions,
  validationText,
} from "./protocol-package-model";
import { version } from "./protocol-packages-test-support";

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
      capabilities: {
        upstream: { frame: true, decode: true, encode: true },
        downstream: { frame: true, decode: true, encode: true },
        display: true,
      },
      schema: { id: "iso8583", version: 1, title: "ISO", fields: [] },
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
});
