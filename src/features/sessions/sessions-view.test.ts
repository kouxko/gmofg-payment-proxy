/** HeroUI 日期值与 Rust 分钟精度筛选文本之间的转换测试。 */

import { describe, expect, it } from "vitest";
import {
  sessionFilterDateText,
  sessionFilterDateValue,
  sessionDetailTabLabels,
} from "./sessions-view";

describe("HeroUI session date filters", () => {
  it("preserves the existing minute-precision query format", () => {
    const value = sessionFilterDateValue("2026-07-29T12:30");

    expect(value).not.toBeNull();
    expect(sessionFilterDateText(value)).toBe("2026-07-29T12:30");
  });

  it("returns an empty filter for invalid or cleared values", () => {
    expect(sessionFilterDateValue("not-a-date")).toBeNull();
    expect(sessionFilterDateText(null)).toBeNull();
  });
});

describe("session detail tabs", () => {
  it("keeps headers inside the three content tabs", () => {
    expect(Object.values(sessionDetailTabLabels)).toEqual(["概览", "请求", "响应"]);
  });
});
