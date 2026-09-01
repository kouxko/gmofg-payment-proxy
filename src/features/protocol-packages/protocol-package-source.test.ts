import { describe, expect, it } from "vitest";
import {
  isBuiltInPackage,
  isExternalPackage,
  isManagedPackage,
  isProtocolPackageSource,
  packageSourceText,
} from "./protocol-package-source";
import { version } from "./protocol-packages-test-support";

describe("protocol package source closed union", () => {
  it("accepts every valid variant and renders every source state", () => {
    const userInstalled = version("1.0.0", {
      package_source: { type: "managed", online: true },
    });
    const builtIn = version("1.0.0", {
      package: { id: "iso8583-ascii-standard", version: "1.0.0" },
    });
    const externalOnline = version("1.0.0", {
      package_source: { type: "external", online: true },
    });
    const externalOffline = version("1.0.0", {
      package_source: { type: "external", online: false },
    });

    expect(isProtocolPackageSource(userInstalled.package_source)).toBe(true);
    expect(isProtocolPackageSource(externalOnline.package_source)).toBe(true);
    expect(packageSourceText(userInstalled)).toBe("本地管理 · 运行中");
    expect(packageSourceText(externalOnline)).toBe("远端调试 · 在线");
    expect(packageSourceText(externalOffline)).toBe("远端调试 · 离线");
    expect(isBuiltInPackage(userInstalled)).toBe(false);
    expect(isBuiltInPackage(builtIn)).toBe(true);
    expect(isBuiltInPackage(externalOnline)).toBe(false);
    expect(isExternalPackage(externalOnline)).toBe(true);
    expect(isExternalPackage(userInstalled)).toBe(false);
    expect(isManagedPackage(userInstalled)).toBe(true);
  });

  it("rejects malformed, extended and unknown variants", () => {
    for (const malformed of [
      null,
      [],
      { type: "internal" },
      { type: "internal", built_in: "yes" },
      { type: "internal", built_in: false, online: false },
      { type: "external" },
      { type: "external", online: "yes" },
      { type: "external", online: false, built_in: false },
      { type: "managed" },
      { type: "managed", online: "yes" },
      { type: "managed", online: false, runtime: "wasm" },
      { type: "legacy", built_in: false },
    ]) {
      expect(isProtocolPackageSource(malformed)).toBe(false);
    }
  });
});
