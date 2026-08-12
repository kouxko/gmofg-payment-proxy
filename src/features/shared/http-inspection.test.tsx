// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MessageContentViewModel } from "@/generated/rust-types";
import {
  HttpBodyViewer,
  HttpRequestTargetView,
} from "./http-inspection";

const jsonMessage: MessageContentViewModel = {
  http_status: null,
  headers: { "content-type": ["application/json; charset=utf-8"] },
  body_text: '{"result":{"code":"D48"}}',
  body_bytes: [123, 34, 114, 101, 115, 117, 108, 116, 34, 58, 123, 125, 125],
  json: { result: { code: "D48" } },
  content_length: 25,
  media_type: "application/json",
  charset: "utf-8",
  content_kind: "json",
  codec_id: "utf-8",
  decode_error: null,
};

describe("HTTP 报文共享查看器", () => {
  it("显示 Rust 识别元数据、行号和 JSON token", () => {
    const { container } = render(
      <HttpBodyViewer label="请求 Body" message={jsonMessage} emptyText="无正文" />,
    );

    expect(screen.getByText("application/json")).toBeVisible();
    expect(screen.getByText("charset=utf-8")).toBeVisible();
    expect(screen.getByText("codec=utf-8")).toBeVisible();
    expect(screen.getByText("JSON")).toBeVisible();
    expect(screen.getByText("1", { selector: "[data-line-number]" })).toBeVisible();
    expect(container.querySelector('[data-token="json-key"]')).toHaveTextContent("result");
    expect(container.querySelector('[data-token="json-string"]')).toHaveTextContent("D48");
    expect(container.querySelector("[dangerouslySetInnerHTML]")).toBeNull();
  });

  it("把自动解析结果显示为用户可读的编码来源", () => {
    render(
      <HttpBodyViewer
        label="响应 Body"
        message={{ ...jsonMessage, codec_id: "auto:shift-jis" }}
        emptyText="无正文"
      />,
    );

    expect(screen.getByText("codec=自动 → Shift-JIS")).toBeVisible();
  });

  it("Header 标成普通文本但正文是 JSON 时，仅为展示生成格式化预览", () => {
    const { container } = render(
      <HttpBodyViewer
        label="响应 Body"
        message={{
          ...jsonMessage,
          headers: { "content-type": ["text/csv; charset=shift_jis"] },
          body_text: '{"ErrorCode":"D48","ResponseID":"A"}',
          json: null,
          media_type: "text/csv",
          charset: "shift_jis",
          content_kind: "text",
          codec_id: "shift-jis",
        }}
        emptyText="无正文"
      />,
    );

    expect(screen.getByText("文本")).toBeVisible();
    expect(screen.getByText("格式化为 JSON 展示")).toBeVisible();
    const formattedLines = Array.from(
      container.querySelectorAll('[data-code-surface="json"] code'),
      (line) => line.textContent,
    );
    expect(formattedLines).toEqual([
      "{",
      '  "ErrorCode": "D48",',
      '  "ResponseID": "A"',
      "}",
    ]);
  });

  it("按 Content-Type 展示 XML，高亮标签且保留解码失败信息", () => {
    const { container } = render(
      <HttpBodyViewer
        label="响应 Body"
        message={{
          ...jsonMessage,
          headers: { "content-type": ["application/xml; charset=shift_jis"] },
          body_text: "<response><code>D48</code></response>",
          json: null,
          media_type: "application/xml",
          charset: "shift_jis",
          content_kind: "xml",
          codec_id: "shift-jis",
          decode_error: "末尾存在无法解码的字节",
        }}
        emptyText="无正文"
      />,
    );

    expect(screen.getByText("XML")).toBeVisible();
    expect(screen.getByText("末尾存在无法解码的字节")).toBeVisible();
    expect(container.querySelector('[data-token="xml-tag"]')).toHaveTextContent("response");
  });

  it("二进制内容明确区分正文与原始字节", () => {
    render(
      <HttpBodyViewer
        label="响应 Body"
        message={{
          ...jsonMessage,
          body_text: null,
          body_bytes: [0, 255, 65],
          json: null,
          media_type: "application/octet-stream",
          charset: null,
          content_kind: "binary",
          codec_id: "raw",
          decode_error: null,
        }}
        emptyText="无正文"
      />,
    );

    expect(screen.getByText("二进制")).toBeVisible();
    expect(screen.getByText("原始字节（3 bytes）")).toBeVisible();
    expect(screen.getByText(/00 ff 41/i)).toBeVisible();
  });

  it("文本正文使用可读的主展示高度，原始字节默认折叠", () => {
    const { container } = render(
      <HttpBodyViewer label="请求 Body" message={jsonMessage} emptyText="无正文" />,
    );

    expect(container.querySelector("[data-code-surface]")).toHaveClass("min-h-64");
    expect(container.querySelector("details")).not.toHaveAttribute("open");
  });

  it("大正文只创建有界预览并说明完整原始数据仍保留", () => {
    const oversized = `${"x".repeat(256 * 1024)}TAIL`;
    render(
      <HttpBodyViewer
        label="响应 Body"
        message={{
          ...jsonMessage,
          body_text: oversized,
          body_bytes: Array.from({ length: 64 * 1024 + 1 }, () => 120),
          json: null,
          media_type: "text/plain",
          content_kind: "text",
        }}
        emptyText="无正文"
      />,
    );

    expect(screen.getByText(/仅渲染安全预览/)).toBeVisible();
    expect(screen.getByText(/总计 65537 bytes，预览 65536 bytes/)).toBeVisible();
    expect(screen.queryByText(/TAIL/)).not.toBeInTheDocument();
  });
});

describe("HTTP 请求目标共享查看器", () => {
  it.each(["GET", "POST", "PUT", "DELETE", "QUERY"])(
    "不限制 %s method，并原样显示 query",
    (method) => {
      const { unmount } = render(
        <HttpRequestTargetView
          method={method}
          target="/pay?code=D48&code=D49&name=A%2BB"
          queryString="code=D48&code=D49&name=A%2BB"
        />,
      );

      expect(screen.getByText(method)).toBeVisible();
      expect(screen.getByText("/pay?code=D48&code=D49&name=A%2BB")).toBeVisible();
      expect(screen.getByText("code=D48&code=D49&name=A%2BB")).toBeVisible();
      unmount();
    },
  );
});
