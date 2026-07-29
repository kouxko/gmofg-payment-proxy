import { describe, expect, it } from "vitest";
import {
  navigation,
  sideNavigationIconClassName,
  sideNavigationItemClassName,
  sideNavigationLabelClassName,
} from "./app-shell";

describe("UI-001 fixed navigation order", () => {
  it("matches the frozen requirement document", () => {
    expect(navigation.map((item) => item.href)).toEqual([
      "/console",
      "/capture",
      "/sessions",
      "/breakpoints",
      "/rules",
      "/faults",
      "/certificates",
      "/settings",
    ]);
  });
});

describe("side navigation alignment", () => {
  it("centers the shared item box within the sidebar content box", () => {
    expect(sideNavigationItemClassName).toContain("mx-auto");
    expect(sideNavigationItemClassName).toContain("!w-[calc(100%_-_1rem)]");
    expect(sideNavigationItemClassName).toContain("items-center");
    expect(sideNavigationItemClassName).toContain("justify-center");
    expect(sideNavigationItemClassName).toContain("text-center");
  });

  it("uses the same centered icon and label contract for links and About", () => {
    expect(sideNavigationIconClassName).toContain("self-center");
    expect(sideNavigationIconClassName).toContain("shrink-0");
    expect(sideNavigationLabelClassName).toContain("w-14");
    expect(sideNavigationLabelClassName).toContain("shrink-0");
    expect(sideNavigationLabelClassName).toContain("whitespace-nowrap");
    expect(sideNavigationLabelClassName).toContain("text-center");
  });
});
