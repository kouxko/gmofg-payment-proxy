import { describe, expect, it } from "vitest";
import type {
  BreakpointActionOptionViewModel,
  BreakpointDraft,
} from "@/generated/rust-types";
import { buildBreakpointDecision } from "./breakpoints-view";

const draft: BreakpointDraft = {
  breakpoint_id: "breakpoint-1",
  expected_revision: 7,
  message: {
    http_status: null,
    headers: { "content-type": ["application/json"] },
    body_text: '{"amount":100}',
    body_bytes: [123, 125],
    json: { amount: 100 },
    content_length: 14,
  },
};

const parameters = {
  delayMs: 1200,
  httpStatus: 503,
  contentLengthDelta: -2,
  truncateAt: 8,
};
const option = (
  kind: BreakpointActionOptionViewModel["kind"],
): BreakpointActionOptionViewModel => ({
  kind,
  label: kind,
  enabled: true,
  disabled_reason: null,
  default_delay_ms: kind === "delay" ? 1000 : null,
  default_http_status: kind === "custom_http_status" ? 503 : null,
  default_content_length_delta:
    kind === "wrong_content_length" ? 1 : null,
  default_truncate_at: kind === "truncate" ? 1 : null,
});

describe("buildBreakpointDecision", () => {
  it("passes the draft and user parameters through for Rust to interpret", () => {
    expect(
      buildBreakpointDecision(draft, option("delay"), parameters),
    ).toMatchObject({
      message: draft.message,
      delay_ms: 1200,
      http_status: 503,
      content_length_delta: -2,
      truncate_at: 8,
    });
    expect(
      buildBreakpointDecision(
        draft,
        option("custom_http_status"),
        parameters,
      ),
    ).toMatchObject({
      message: draft.message,
      delay_ms: 1200,
      http_status: 503,
    });
    expect(
      buildBreakpointDecision(
        draft,
        option("wrong_content_length"),
        parameters,
      ),
    ).toMatchObject({
      message: draft.message,
      content_length_delta: -2,
    });
    expect(
      buildBreakpointDecision(draft, option("truncate"), parameters),
    ).toMatchObject({
        message: draft.message,
        truncate_at: 8,
      });
  });
});
