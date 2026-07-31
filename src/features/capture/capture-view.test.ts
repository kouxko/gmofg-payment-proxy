/** 抓包查询草稿和工作区跳转的纯函数合约测试。 */

import { describe, expect, it } from "vitest";
import {
  captureDetailTabLabels,
  defaultCaptureQuery,
  resumeCaptureQuery,
  ruleEditorHref,
} from "./capture-view";

describe("CAPTURE detail navigation", () => {
  it("keeps HTTP status and Header metadata out of the compact tab labels", () => {
    expect(Object.values(captureDetailTabLabels)).toEqual([
      "概览",
      "请求",
      "响应",
    ]);
    expect(Object.values(captureDetailTabLabels).join(" ")).not.toMatch(
      /Header|\d{3}/,
    );
  });
});

describe("CAPTURE-009 create rule navigation", () => {
  it("only carries the selected session ID to the Rust-backed rule editor", () => {
    expect(ruleEditorHref("session/id + 1")).toBe(
      "/rules?sessionId=session%2Fid%20%2B%201",
    );
  });
});

describe("CAPTURE-003 pause display", () => {
  it("requests a full Rust display snapshot when scrolling resumes", () => {
    expect(
      resumeCaptureQuery({
        ...defaultCaptureQuery,
        keyword: "2740072778",
        page: { page: 4, page_size: 50 },
      }),
    ).toMatchObject({
      keyword: "2740072778",
      after_event_id: null,
      page: { page: 1, page_size: 50 },
    });
  });
});
