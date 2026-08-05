import { describe, expect, it } from "vitest";
import { formatMessageBody } from "./message-content";

describe("formatMessageBody", () => {
  it("formats the JSON structure parsed by Rust", () => {
    expect(
      formatMessageBody(
        {
          body_text: '{"result":{"code":"D48"}}',
          json: { result: { code: "D48" } },
        },
        "无正文",
      ),
    ).toBe(`{
  "result": {
    "code": "D48"
  }
}`);
  });

  it("preserves non-JSON and invalid JSON body text", () => {
    expect(
      formatMessageBody(
        { body_text: "{invalid", json: null },
        "无正文",
      ),
    ).toBe("{invalid");
  });

  it("uses the supplied empty-state text when no body exists", () => {
    expect(formatMessageBody(undefined, "无正文")).toBe("无正文");
  });
});
