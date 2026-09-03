// @vitest-environment jsdom

/** 不可信协议 HTML 的清洗、隔离、CSP 与资源上限测试。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProtocolSafeDisplay } from "./protocol-safe-display";

async function renderedSource(): Promise<string> {
  const frame = await screen.findByTitle("协议包安全展示");
  await waitFor(() => expect(frame).toHaveAttribute("srcdoc"));
  return frame.getAttribute("srcdoc") ?? "";
}

describe("ProtocolSafeDisplay", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps only the protocol-display allowlist and harmless table attributes", async () => {
    render(
      <ProtocolSafeDisplay
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
      <ProtocolSafeDisplay
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

  it("keeps allowlisted inline visual styles and removes unsafe CSS", async () => {
    render(
      <ProtocolSafeDisplay
        html={'<pre style="background-color:#1e1e1e;color:#d4d4d4;padding:12px;border-radius:8px;font-family:monospace;position:fixed;width:9999px;background-image:url(https://evil.test/x)"><span style="color:#9cdcfe;font-weight:700;transform:scale(20)">key</span></pre>'}
      />,
    );

    const source = await renderedSource();
    expect(source).toContain('style="background-color: rgb(30, 30, 30); color: rgb(212, 212, 212); padding: 12px; border-radius: 8px; font-family: monospace;"');
    expect(source).toContain('style="color: rgb(156, 220, 254); font-weight: 700;"');
    expect(source).not.toMatch(/position:\s*fixed|width:\s*9999|background-image:|transform:\s*scale|evil\.test/i);
  });

  it("preserves distinct safe colors for separately rendered protocol tables", async () => {
    render(
      <ProtocolSafeDisplay
        html={`
          <table style="background-color:hsl(18 45% 46% / 0.10);border-color:hsl(18 65% 50%)"><caption style="background-color:hsl(18 45% 46% / 0.22)">A</caption><tbody><tr><td>1</td></tr></tbody></table>
          <table style="background-color:hsl(155 45% 46% / 0.10);border-color:hsl(155 65% 50%)"><caption style="background-color:hsl(155 45% 46% / 0.22)">B</caption><tbody><tr><td>2</td></tr></tbody></table>
        `}
      />,
    );

    const source = await renderedSource();
    const safeDocument = new DOMParser().parseFromString(source, "text/html");
    const tables = Array.from(safeDocument.querySelectorAll("table"));
    expect(tables).toHaveLength(2);
    expect(tables[0].style.backgroundColor).not.toBe("");
    expect(tables[0].style.borderColor).not.toBe("");
    expect(tables[0].style.backgroundColor).not.toBe(tables[1].style.backgroundColor);
    expect(tables[0].querySelector("caption")?.style.backgroundColor).not.toBe(
      tables[1].querySelector("caption")?.style.backgroundColor,
    );
  });

  it("keeps wide tables readable inside an independent horizontal scroller", async () => {
    render(
      <ProtocolSafeDisplay
        html={'<section style="white-space:pre-wrap;overflow-wrap:anywhere"><table><thead><tr><th>brand_individual_info</th></tr></thead><tbody><tr><td>999999990001000</td></tr></tbody></table></section>'}
      />,
    );

    const source = await renderedSource();
    const safeDocument = new DOMParser().parseFromString(source, "text/html");
    const scroller = safeDocument.querySelector(".protocol-display-scroll");
    const table = scroller?.firstElementChild;
    expect(scroller?.parentElement?.tagName).toBe("SECTION");
    expect(table?.tagName).toBe("TABLE");
    expect(table?.classList.contains("protocol-display-table")).toBe(true);
    expect(source).toContain(".protocol-display-scroll{max-width:100%;overflow-x:auto;");
    expect(source).toContain("width:max-content;min-width:100%");
    expect(source).toContain("white-space:pre;overflow-wrap:normal;word-break:normal");
  });

  it("preserves a native collapsible JSON tree without event capabilities", async () => {
    render(
      <ProtocolSafeDisplay
        html={`
          <details open ontoggle="alert(1)">
            <summary><strong>基本信息</strong><span>Object · 2 fields</span></summary>
            <details>
              <summary><strong>KCCI_01</strong><span>Array · 1 items</span></summary>
              <table><tbody><tr><td>safe</td></tr></tbody></table>
            </details>
          </details>
        `}
      />,
    );

    const source = await renderedSource();
    const safeDocument = new DOMParser().parseFromString(source, "text/html");
    const nodes = Array.from(safeDocument.querySelectorAll("details"));
    expect(nodes).toHaveLength(2);
    expect(nodes[0].open).toBe(true);
    expect(nodes[1].open).toBe(false);
    expect(nodes[0].querySelector(":scope > summary")?.textContent).toBe("基本信息Object · 2 fields");
    expect(nodes[1].querySelector(":scope > summary")?.textContent).toBe("KCCI_01Array · 1 items");
    expect(source).toContain(".protocol-display-tree-node");
    expect(source).toContain(".protocol-display-tree-summary");
    expect(source).not.toMatch(/ontoggle|alert\(1\)/i);
  });

  it("uses a no-capability iframe with an inner deny-by-default CSP", async () => {
    render(<ProtocolSafeDisplay html="<p>safe</p>" />);

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

  it("renders an HTML source above the former 128 KiB limit", async () => {
    render(<ProtocolSafeDisplay html={`<p>${"x".repeat(512 * 1024)}</p>`} />);

    expect(await screen.findByTitle("协议包安全展示")).toBeVisible();
    expect(screen.queryByText(/协议视图超过/)).not.toBeInTheDocument();
  });

  it("refuses an HTML source over 1 MiB instead of rendering a truncated document", async () => {
    render(<ProtocolSafeDisplay html={`<p>${"x".repeat(1024 * 1024)}</p>`} />);

    expect(
      await screen.findByText("协议视图超过 1 MiB，已禁止渲染"),
    ).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
    expect(screen.getByText("原始字节仍保留在当前 Exchange 记录中。")).toBeVisible();
  });

  it("renders a flat table above the former 4096-node limit", async () => {
    render(
      <ProtocolSafeDisplay
        html={`<table><tbody>${"<tr><td>x</td></tr>".repeat(1_500)}</tbody></table>`}
      />,
    );

    expect(await screen.findByTitle("协议包安全展示")).toBeVisible();
    expect(screen.queryByText("协议视图结构过于复杂，已禁止渲染")).not.toBeInTheDocument();
  });

  it("rejects a flat table above the 8192-node limit", async () => {
    render(
      <ProtocolSafeDisplay
        html={`<table><tbody>${"<tr><td>x</td></tr>".repeat(3_000)}</tbody></table>`}
      />,
    );

    expect(await screen.findByText("协议视图结构过于复杂，已禁止渲染")).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
  });

  it("rejects deeply nested HTML without overflowing the sanitizer stack", async () => {
    render(<ProtocolSafeDisplay html={`${"<div>".repeat(3_000)}safe${"</div>".repeat(3_000)}`} />);

    expect(await screen.findByText("协议视图结构过于复杂，已禁止渲染")).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
    expect(screen.getByText("原始字节仍保留在当前 Exchange 记录中。")).toBeVisible();
  });

  it("fails closed when the browser parser itself rejects the untrusted document", async () => {
    vi.stubGlobal("DOMParser", class {
      constructor() {
        throw new Error("parser unavailable");
      }
    });

    render(<ProtocolSafeDisplay html="<p>untrusted</p>" />);

    expect(await screen.findByText("协议视图结构过于复杂，已禁止渲染")).toBeVisible();
    expect(screen.queryByTitle("协议包安全展示")).not.toBeInTheDocument();
  });
});
