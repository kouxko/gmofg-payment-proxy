import { describe, expect, it } from "vitest";
import { formatTimestamp } from "./format";

describe("formatTimestamp", () => {
  it("normalizes equivalent instants before formatting in local time", () => {
    expect(formatTimestamp("2026-08-19T05:21:32.015Z"))
      .toBe(formatTimestamp("2026-08-19T13:21:32.015+08:00"));
  });

  it("keeps millisecond precision and rejects invalid values", () => {
    expect(formatTimestamp("2026-08-19T05:21:32.015Z"))
      .toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.015$/);
    expect(formatTimestamp("not-a-timestamp")).toBe("—");
    expect(formatTimestamp()).toBe("—");
  });
});
