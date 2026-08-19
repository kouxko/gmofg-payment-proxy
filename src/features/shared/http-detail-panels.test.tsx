// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  CaptureDetailViewModel,
  CaptureRowViewModel,
  MessageContentViewModel,
  SessionSummaryViewModel,
} from "@/generated/rust-types";
import { CaptureDetailPanel } from "@/features/capture/capture-detail-panel";

const message: MessageContentViewModel = {
  protocol: null,
  protocol_failure: null,
  http_status: null,
  start_line_bytes: [],
  raw_headers: [],
  headers: { "content-type": ["application/xml; charset=utf-8"] },
  body_text: "<request><code>D48</code></request>",
  body_bytes: [60, 114, 101, 113, 117, 101, 115, 116, 62],
  json: null,
  content_length: 37,
  media_type: "application/xml",
  charset: "utf-8",
  content_kind: "xml",
  codec_id: "utf-8",
  decode_error: null,
  query_string: "code=D48&name=A%2BB",
};

const summary: SessionSummaryViewModel = {
  session_id: "session-1",
  request_id: "request-1",
  started_at: "2026-08-11T00:00:00Z",
  completed_at: null,
  terminal_ip: "10.0.0.2",
  channel: "transaction",
  channel_text: "交易",
  method: "QUERY",
  target: "/pay?code=D48&name=A%2BB",
  http_status: null,
  result: "处理中",
  ui_tone: "info",
  duration_ms: null,
  matched_rule_ids: [],
  request_size_bytes: 37,
  response_size_bytes: 0,
  pending_breakpoint: false,
  revision: 1,
};

describe("抓包正文接入共享查看器", () => {
  it("抓包请求页展示任意 method、原始 query 与 XML", async () => {
    const user = userEvent.setup();
    const selected: CaptureRowViewModel = {
      event_id: 1,
      runtime_epoch: "epoch-1",
      occurred_at: "2026-08-11T00:00:00Z",
      stage: "request",
      stage_text: "请求",
      size_bytes: 37,
      breakpoint_id: null,
      can_go_to_breakpoint: false,
      breakpoint_disabled_reason: null,
      ...summary,
    };
    const detail = {
      data: {
        session_id: "session-1",
        request_id: "request-1",
        terminal_ip: "10.0.0.2",
        certificate_fingerprint_suffix: "ABCD",
        upstream_host: "server.test",
        request: message,
        response: null,
        tls_summary: "TLS 1.2",
        timings_ms: {},
        rule_trace: [],
        revision: 1,
      } satisfies CaptureDetailViewModel,
      isLoading: false,
      refresh: vi.fn(),
      invalidate: vi.fn(),
    };

    render(
      <CaptureDetailPanel
        selected={selected}
        detail={detail}
        requestHeaderCount={1}
        responseHeaderCount={0}
        onClose={vi.fn()}
        onNavigate={vi.fn()}
        onCreateRule={vi.fn()}
      />,
    );
    expect(screen.getByRole("dialog", { name: "抓包详情" })).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "请求" }));

    expect(screen.getByText("QUERY")).toBeVisible();
    expect(screen.getByText("code=D48&name=A%2BB")).toBeVisible();
    expect(screen.getByText("XML")).toBeVisible();
  });

});
