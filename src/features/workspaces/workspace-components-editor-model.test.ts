import { describe, expect, it } from "vitest";
import {
  extractorSourceValue,
  faultActionLabel,
  faultActionValue,
  updateAtIndex,
  updateExtractorSource,
  updateFaultAction,
} from "./workspace-components-editor-model";

describe("workspace component editor helpers", () => {
  it("updates list items without touching unrelated entries", () => {
    expect(updateAtIndex(["a", "b", "c"], 1, (value) => value.toUpperCase())).toEqual([
      "a",
      "B",
      "c",
    ]);
  });

  it("keeps extractor source value mapping symmetric for editable variants", () => {
    const header = { kind: "header" as const, name: "X-Test" };
    const path = { kind: "json_path" as const, path: "$.data" };
    const fixed = { kind: "fixed_value" as const, value: "ready" };

    expect(extractorSourceValue(header)).toBe("X-Test");
    expect(extractorSourceValue(path)).toBe("$.data");
    expect(extractorSourceValue(fixed)).toBe("ready");
    expect(updateExtractorSource(header, "X-New")).toEqual({
      kind: "header",
      name: "X-New",
    });
    expect(updateExtractorSource(path, "$.value")).toEqual({
      kind: "json_path",
      path: "$.value",
    });
    expect(updateExtractorSource(fixed, "done")).toEqual({
      kind: "fixed_value",
      value: "done",
    });
    const bodyText = { kind: "body_text" as const };
    expect(extractorSourceValue(bodyText)).toBe("");
    expect(updateExtractorSource(bodyText, "ignored")).toBe(bodyText);
  });

  it.each([
    [{ kind: "delay" as const, milliseconds: 200 }, 200, "毫秒", 300, { kind: "delay", milliseconds: 300 }],
    [{ kind: "idle_timeout" as const, milliseconds: 400 }, 400, "毫秒", 500, { kind: "idle_timeout", milliseconds: 500 }],
    [{ kind: "rate_limit" as const, bytes_per_second: 1024 }, 1024, "字节/秒", 2048, { kind: "rate_limit", bytes_per_second: 2048 }],
    [{ kind: "close_after_bytes" as const, bytes: 64 }, 64, "字节数", 128, { kind: "close_after_bytes", bytes: 128 }],
    [{ kind: "half_close_after_bytes" as const, bytes: 96 }, 96, "字节数", 192, { kind: "half_close_after_bytes", bytes: 192 }],
  ] as const)("maps the %s fault action", (action, value, label, next, updated) => {
    expect(faultActionValue(action)).toBe(value);
    expect(faultActionLabel(action)).toBe(label);
    expect(updateFaultAction(action, next)).toEqual(updated);
  });

  it("keeps the parameterless reject action unchanged", () => {
    const reject = { kind: "reject" as const };
    expect(faultActionValue(reject)).toBe(0);
    expect(updateFaultAction(reject, 1)).toBe(reject);
  });
});
