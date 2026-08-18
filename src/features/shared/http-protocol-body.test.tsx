// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { Document, MessageContentViewModel } from "@/generated/rust-types";
import { HttpProtocolBodyViewer } from "./http-protocol-body";

const asciiBytes = (value: string) => Array.from(value, (character) => character.charCodeAt(0));

function document(value: string): Document {
  return {
    schema: {
      id: "http-upstream",
      version: 1,
      title: "支付请求字段",
      fields: [{ name: "amount", type: "string", label: "金额" }],
    },
    values: [{ type: "string", value }],
  };
}

const message: MessageContentViewModel = {
  http_status: null,
  start_line_bytes: [],
  raw_headers: [],
  headers: {},
  body_text: "{\"amount\":\"0\"}",
  body_bytes: [123, 125],
  json: null,
  content_length: 2,
  media_type: "application/json",
  charset: "utf-8",
  content_kind: "json",
  codec_id: "utf-8",
  decode_error: null,
  query_string: null,
  protocol: {
    package: { id: "http-json", version: "1.0.0" },
    origin_body: asciiBytes('{"amount":"0"}'),
    origin_text: '{"amount":"0"}',
    written_body: asciiBytes('{"amount":"200"}'),
    written_text: '{"amount":"200"}',
    document: document("200"),
    display: { kind: "hex_fallback", reason: "entry_point_failed" },
    stages: [
      {
        stage: "app_to_proxy",
        matched_rule_ids: ["rule-1"],
        document: document("100"),
        display: { kind: "hex_fallback", reason: "entry_point_failed" },
      },
      {
        stage: "proxy_to_upstream",
        matched_rule_ids: ["rule-2"],
        document: document("200"),
        display: { kind: "hex_fallback", reason: "entry_point_failed" },
      },
    ],
  },
  protocol_failure: null,
};

describe("HTTP 协议 Body 四阶段证据", () => {
  it("分别展示应用到代理与代理到上游服务的阶段快照", async () => {
    const user = userEvent.setup();
    render(<HttpProtocolBodyViewer label="请求 Body" message={message} emptyText="无正文" />);

    expect(screen.getByRole("tab", { name: "应用 → 代理" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "最终协议视图" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("200")).toBeVisible();
    expect(screen.getByText("命中 2 条规则")).toBeVisible();
    expect(screen.queryByText("entry_point_failed")).not.toBeInTheDocument();
    expect(screen.getByText("协议视图生成失败，请查看原始或写出 Body。")).toBeVisible();

    await user.click(screen.getByRole("tab", { name: "代理 → 上游服务" }));
    expect(screen.getByText("200")).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "应用 → 代理" }));
    expect(screen.getByText("100")).toBeVisible();
    expect(screen.queryByText("200")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "原始 Body" }));
    expect(screen.getByText('{"amount":"0"}')).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "写出 Body" }));
    expect(screen.getByText('{"amount":"200"}')).toBeVisible();
  });

  it("分别展示下行两个阶段并合计每个阶段的命中规则", async () => {
    const user = userEvent.setup();
    const downstream: MessageContentViewModel = {
      ...message,
      protocol: {
        ...message.protocol!,
        document: document("220"),
        display: { kind: "untrusted_html", html: "<p>响应金额 220</p>" },
        stages: [
          {
            stage: "upstream_to_proxy",
            matched_rule_ids: ["response-1", "response-2"],
            document: document("210"),
            display: { kind: "hex_fallback", reason: "resource_limit_exceeded" },
          },
          {
            stage: "proxy_to_app",
            matched_rule_ids: ["response-3"],
            document: document("220"),
            display: { kind: "untrusted_html", html: "<table><tr><td>220</td></tr></table>" },
          },
        ],
      },
    };

    render(<HttpProtocolBodyViewer label="响应 Body" message={downstream} emptyText="无正文" />);
    expect(screen.getByRole("tab", { name: "上游服务 → 代理" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "最终协议视图" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("命中 3 条规则")).toBeVisible();
    expect(await screen.findByTitle("协议包安全展示")).toBeVisible();

    await user.click(screen.getByRole("tab", { name: "上游服务 → 代理" }));
    expect(screen.getByText("协议视图超过处理限制，请查看原始或写出 Body。")).toBeVisible();
    expect(screen.queryByText("resource_limit_exceeded")).not.toBeInTheDocument();
  });

  it("协议处理证据为空时沿用普通 HTTP Body 展示", async () => {
    const plain: MessageContentViewModel = {
      ...message,
      protocol: null,
      body_text: "plain body",
      body_bytes: asciiBytes("plain body"),
    };

    render(<HttpProtocolBodyViewer label="请求 Body" message={plain} emptyText="无正文" />);
    expect(screen.getByText("plain body")).toBeVisible();
    await waitFor(() => expect(screen.queryByLabelText("请求 Body协议处理结果")).not.toBeInTheDocument());
    expect(screen.queryByRole("tab", { name: "原始 Body" })).not.toBeInTheDocument();
  });

  it("展示持久化的协议失败码、阶段与安全详情", () => {
    const failed: MessageContentViewModel = {
      ...message,
      protocol: null,
      protocol_failure: {
        package: { id: "http-json", version: "1.0.0" },
        direction: "upstream",
        stage: "proxy_to_upstream",
        kind: "rule_failed",
        code: "DOCUMENT_TRANSFORM_FAILED",
        detail: "协议报文规则执行失败",
        origin_body: asciiBytes('{"amount":"0"}'),
      },
    };

    render(<HttpProtocolBodyViewer label="请求 Body" message={failed} emptyText="无正文" />);
    expect(screen.getByLabelText("请求 Body协议处理失败")).toBeVisible();
    expect(screen.getByText(/DOCUMENT_TRANSFORM_FAILED/)).toBeVisible();
    expect(screen.getByText(/代理 → 上游服务/)).toBeVisible();
  });
});
