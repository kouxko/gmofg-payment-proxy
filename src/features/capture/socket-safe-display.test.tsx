// @vitest-environment jsdom

/** 不可信协议 HTML 的清洗、隔离、CSP 与资源上限测试。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SocketSafeDisplay } from "./socket-safe-display";

async function renderedSource(): Promise<string> {
  const frame = await screen.findByTitle("协议包安全展示");
  await waitFor(() => expect(frame).toHaveAttribute("srcdoc"));
  return frame.getAttribute("srcdoc") ?? "";
}

describe("SocketSafeDisplay", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps only the protocol-display allowlist and harmless table attributes", async () => {
    render(
      <SocketSafeDisplay
        html={'<article title="summary"><table class="package-owned"><thead><tr><th>Field</th></tr></thead><tbody><tr><td colspan="2">OK</td></tr></tbody></table><unknown>kept text</unknown></article>'}
      />,
    );

    const source = await renderedSource();
    expect(source).toContain('<article title="summary">');
    expect(source).toContain('<table class="protocol-display-table">');
    expect(source).toContain('<thead class="protocol-display-head">');
    expect(source).toContain('<th class="protocol-display-header">Field</th>');
    expect(source).toContain('<td class="protocol-display-cell" colspan="2">OK</td>');
    expect(source).not.toContain("package-owned");
    expect(source).toContain("kept text");
    expect(source).not.toContain("<unknown");
  });

  it("removes active content, event handlers, navigation and external loads", async () => {
    render(
      <SocketSafeDisplay
        html={`
          <script>parent.__TAURI_INTERNALS__.invoke("evil")</script>
          <img src="https://evil.test/pixel" onerror="alert(1)">
          <svg onload="alert(2)"><a href="javascript:alert(3)">x</a></svg>
          <math><a href="data:text/html,evil">math</a></math>
          <iframe srcdoc="<script>alert(4)</script>"></iframe>
          <object data="https://evil.test/object"></object>
          <embed src="https://evil.test/embed">
          <form action="https://evil.test/post"><input name="secret"></form>
          <meta http-equiv="refresh" content="0;url=https://evil.test">
          <base href="https://evil.test/">
          <link rel="stylesheet" href="https://evil.test/a.css">
          <style>@import "https://evil.test/x.css";p{background:url(https://evil.test/x)}</style>
          <p onclick="alert(5)" style="background:url(https://evil.test/y)"><a href="javascript:alert(6)">safe text</a></p>
        `}
      />,
    );

    const source = await renderedSource();
    expect(source).toContain("safe text");
    expect(source).not.toMatch(/__TAURI_INTERNALS__|evil\.test|javascript:|data:text\/html/i);
    expect(source).not.toMatch(/onerror|onload|onclick|srcdoc|@import/i);
    expect(source).not.toMatch(/<script|<img|<svg|<math|<iframe|<object|<embed|<form|<input|<meta[^>]+refresh|<base|<link/i);
  });

  it("uses a no-capability iframe with an inner deny-by-default CSP", async () => {
    render(<SocketSafeDisplay html="<p>safe</p>" />);

    const frame = await screen.findByTitle("协议包安全展示");
    const source = await renderedSource();
    expect(frame).toHaveAttribute("sandbox", "");
    expect(frame).toHaveAttribute("referrerpolicy", "no-referrer");
    expect(source).toContain("default-src 'none'");
    expect(source).toContain("script-src 'none'");
    expect(source).toContain("connect-src 'none'");
    expect(source).toContain("img-src 'none'");
    expect(source).toContain("object-src 'none'");
    expect(source).toContain("frame-src 'none'");
    expect(source).toContain("form-action 'none'");
    expect(source).toContain("base-uri 'none'");
  });

  it("refuses an HTML source over 128 KiB instead of rendering a truncated document", async () => {
    render(<SocketSafeDisplay html={`<p>${"x".repeat(128 * 1024)}</p>`} />);

    expect(
      await screen.findByText("协议视图超过 128 KiB，已禁止渲染"),
    ).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
    expect(screen.getByText("完整字节仍可在 Hex 页签逐页查看。")).toBeVisible();
  });

  it("rejects deeply nested HTML without overflowing the sanitizer stack", async () => {
    render(<SocketSafeDisplay html={`${"<div>".repeat(3_000)}safe${"</div>".repeat(3_000)}`} />);

    expect(await screen.findByText("协议视图结构过于复杂，已禁止渲染")).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
    expect(screen.getByText("完整字节仍可在 Hex 页签逐页查看。")).toBeVisible();
  });

  it("fails closed when the browser parser itself rejects the untrusted document", async () => {
    vi.stubGlobal("DOMParser", class {
      constructor() {
        throw new Error("parser unavailable");
      }
    });

    render(<SocketSafeDisplay html="<p>untrusted</p>" />);

    expect(await screen.findByText("协议视图结构过于复杂，已禁止渲染")).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
  });
});
