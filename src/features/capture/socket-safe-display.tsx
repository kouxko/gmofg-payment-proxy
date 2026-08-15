"use client";

import { useEffect, useState } from "react";
import { Alert, Spinner } from "@heroui/react";

const DISPLAY_SOURCE_LIMIT = 128 * 1024;
const SAFE_ELEMENTS = new Set([
  "article", "section", "div", "span", "p", "br", "hr", "strong", "em", "b", "i", "code", "pre",
  "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "dl", "dt", "dd",
  "table", "caption", "thead", "tbody", "tfoot", "tr", "th", "td",
]);
const DROP_WITH_CONTENT = new Set([
  "script", "style", "iframe", "object", "embed", "svg", "math", "template", "noscript", "form",
  "input", "button", "select", "textarea", "video", "audio", "canvas", "link", "meta", "base",
]);
const CSP = "default-src 'none'; script-src 'none'; connect-src 'none'; img-src 'none'; media-src 'none'; font-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; style-src 'unsafe-inline'";
const HOST_STYLE = `
:root{color-scheme:light dark;font:14px/1.55 system-ui,sans-serif}body{margin:0;padding:16px;color:#172033;background:transparent}
table{border-collapse:collapse;width:100%}th,td{border:1px solid #ccd3df;padding:6px 8px;text-align:left;vertical-align:top}
pre,code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;white-space:pre-wrap;overflow-wrap:anywhere}
@media(prefers-color-scheme:dark){body{color:#e7ecf3}th,td{border-color:#4c5668}}
`;

export function displayHtmlIsTooLarge(html: string): boolean {
  return new Blob([html]).size > DISPLAY_SOURCE_LIMIT;
}

function cloneSafeNode(node: Node, output: Document): Node | undefined {
  if (node.nodeType === Node.TEXT_NODE) return output.createTextNode(node.textContent ?? "");
  if (!(node instanceof Element)) return undefined;
  const tag = node.tagName.toLowerCase();
  if (DROP_WITH_CONTENT.has(tag)) return undefined;
  const container = SAFE_ELEMENTS.has(tag) ? output.createElement(tag) : output.createDocumentFragment();
  if (container instanceof HTMLElement) {
    const title = node.getAttribute("title");
    if (title) container.setAttribute("title", title.slice(0, 512));
    if (tag === "td" || tag === "th") {
      for (const name of ["colspan", "rowspan"] as const) {
        const raw = node.getAttribute(name);
        if (raw && /^\d{1,2}$/.test(raw) && Number(raw) >= 1 && Number(raw) <= 99) {
          container.setAttribute(name, raw);
        }
      }
    }
  }
  node.childNodes.forEach((child) => {
    const safe = cloneSafeNode(child, output);
    if (safe) container.appendChild(safe);
  });
  return container;
}

function sanitizeDisplay(html: string): string {
  const input = new DOMParser().parseFromString(html, "text/html");
  const output = document.implementation.createHTMLDocument("Socket 协议视图");
  output.documentElement.lang = "zh-CN";
  const meta = output.createElement("meta");
  meta.httpEquiv = "Content-Security-Policy";
  meta.content = CSP;
  output.head.replaceChildren(meta);
  const style = output.createElement("style");
  style.textContent = HOST_STYLE;
  output.head.appendChild(style);
  input.body.childNodes.forEach((node) => {
    const safe = cloneSafeNode(node, output);
    if (safe) output.body.appendChild(safe);
  });
  return `<!doctype html>${output.documentElement.outerHTML}`;
}

export function SocketSafeDisplay({ html }: { html: string }) {
  const [state, setState] = useState<
    { kind: "loading" } | { kind: "too-large"; source: string } | { kind: "ready"; source: string; srcDoc: string }
  >({ kind: "loading" });

  useEffect(() => {
    // DOMParser 和 DOM Node 只在挂载后使用，SSR 不生成不同的 iframe 内容。
    const task = window.setTimeout(() => {
      if (displayHtmlIsTooLarge(html)) setState({ kind: "too-large", source: html });
      else setState({ kind: "ready", source: html, srcDoc: sanitizeDisplay(html) });
    }, 0);
    return () => window.clearTimeout(task);
  }, [html]);

  if (state.kind === "loading" || state.source !== html) {
    return <Spinner aria-label="正在安全解析协议视图" />;
  }
  if (state.kind === "too-large") {
    return (
      <Alert status="warning">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>协议视图超过 128 KiB，已禁止渲染</Alert.Title>
          <Alert.Description>完整字节仍可在 Hex 页签逐页查看。</Alert.Description>
        </Alert.Content>
      </Alert>
    );
  }
  return (
    <iframe
      className="min-h-80 w-full rounded-xl border border-[var(--telemetry-line)] bg-transparent"
      sandbox=""
      referrerPolicy="no-referrer"
      title="Socket 协议安全展示"
      srcDoc={state.srcDoc}
    />
  );
}
