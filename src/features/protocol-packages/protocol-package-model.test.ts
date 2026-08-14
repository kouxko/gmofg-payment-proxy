import { describe, expect, it } from "vitest";
import {
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
});
