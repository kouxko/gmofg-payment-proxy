import { describe, expect, it } from "vitest";
import { isProtocolPackageSchema } from "./protocol-package-schema";

describe("recursive protocol package Schema", () => {
  it("accepts an empty property name because JSON objects permit it", () => {
    expect(isProtocolPackageSchema({
      root: { type: "object", properties: { "": { type: "string" } } },
    })).toBe(true);
  });

  it("does not add an arbitrary frontend recursion-depth limit", () => {
    let root: unknown = { type: "string" };
    for (let index = 0; index < 80; index += 1) {
      root = { type: "array", items: root };
    }
    expect(isProtocolPackageSchema({ root })).toBe(true);
  });
});
