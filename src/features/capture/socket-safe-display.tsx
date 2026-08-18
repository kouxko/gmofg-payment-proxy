"use client";

import { useEffect, useState } from "react";
import { Alert, Spinner } from "@heroui/react";

const DISPLAY_SOURCE_LIMIT = 128 * 1024;
const DISPLAY_NODE_LIMIT = 4_096;
const DISPLAY_DEPTH_LIMIT = 128;
const SAFE_ELEMENTS = new Set([
  "article", "section", "div", "span", "p", "br", "hr", "strong", "em", "b", "i", "code", "pre",
  "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "dl", "dt", "dd",
  "table", "caption", "thead", "tbody", "tfoot", "tr", "th", "td",
]);
const DROP_WITH_CONTENT = new Set([
  "script", "style", "iframe", "object", "embed", "svg", "math", "template", "noscript", "form",
  "input", "button", "select", "textarea", "video", "audio", "canvas", "link", "meta", "base",
]);
const DISPLAY_CLASSES: Partial<Record<string, string>> = {
  table: "protocol-display-table",
  caption: "protocol-display-caption",
  thead: "protocol-display-head",
  tbody: "protocol-display-body",
  tfoot: "protocol-display-foot",
  tr: "protocol-display-row",
  th: "protocol-display-header",
  td: "protocol-display-cell",
};
const CSP = "default-src 'none'; script-src 'none'; connect-src 'none'; img-src 'none'; media-src 'none'; font-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; style-src 'unsafe-inline'";
const HOST_STYLE = `
:root{color-scheme:light dark;font:14px/1.55 system-ui,sans-serif}body{margin:0;padding:16px;color:#172033;background:transparent}
.protocol-display-table{border-collapse:separate;border-spacing:0;width:100%;overflow:hidden;border:1px solid #d5dbe5;border-radius:12px;background:#fff;box-shadow:0 1px 2px rgba(15,23,42,.05)}
.protocol-display-caption{padding:0 0 10px;text-align:left;font-weight:650;color:#334155}
.protocol-display-header,.protocol-display-cell{padding:10px 12px;text-align:left;vertical-align:top;border:0;border-bottom:1px solid #e2e7ef}
.protocol-display-header{background:#f4f6f9;color:#334155;font-weight:650}
.protocol-display-row>:not(:first-child){border-left:1px solid #e2e7ef}
.protocol-display-body .protocol-display-row:nth-child(even) .protocol-display-cell{background:#f8fafc}
.protocol-display-body .protocol-display-row:last-child .protocol-display-cell,.protocol-display-foot .protocol-display-row:last-child>*{border-bottom:0}
pre,code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;white-space:pre-wrap;overflow-wrap:anywhere}
@media(prefers-color-scheme:dark){body{color:#e7ecf3}.protocol-display-table{border-color:#3a414d;background:#171a1f;box-shadow:none}.protocol-display-caption{color:#d8dee8}.protocol-display-header{background:#23272f;color:#f1f4f8}.protocol-display-header,.protocol-display-cell{border-bottom-color:#353b46}.protocol-display-row>:not(:first-child){border-left-color:#353b46}.protocol-display-body .protocol-display-row:nth-child(even) .protocol-display-cell{background:#1d2128}}
`;

export function displayHtmlIsTooLarge(html: string): boolean {
  return new Blob([html]).size > DISPLAY_SOURCE_LIMIT;
}

function createSafeElement(node: Element, output: Document): HTMLElement | undefined {
  const tag = node.tagName.toLowerCase();
  if (DROP_WITH_CONTENT.has(tag)) return undefined;
  if (!SAFE_ELEMENTS.has(tag)) return output.createElement("span");
  const element = output.createElement(tag);
  const displayClass = DISPLAY_CLASSES[tag];
  if (displayClass) element.className = displayClass;
  const title = node.getAttribute("title");
  if (title) element.setAttribute("title", title.slice(0, 512));
  if (tag === "td" || tag === "th") {
    for (const name of ["colspan", "rowspan"] as const) {
      const raw = node.getAttribute(name);
      if (raw && /^\d{1,2}$/.test(raw) && Number(raw) >= 1 && Number(raw) <= 99) {
        element.setAttribute(name, raw);
      }
    }
  }
  return element;
}

function sanitizeDisplay(html: string): string | undefined {
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
  const pending = Array.from(input.body.childNodes, (node) => ({
    node,
    parent: output.body as Node,
    depth: 1,
  })).reverse();
  let visited = 0;
  while (pending.length > 0) {
    const current = pending.pop();
    if (!current) break;
    visited += 1;
    if (visited > DISPLAY_NODE_LIMIT || current.depth > DISPLAY_DEPTH_LIMIT) return undefined;
    if (current.node.nodeType === Node.TEXT_NODE) {
      current.parent.appendChild(output.createTextNode(current.node.textContent ?? ""));
      continue;
    }
    if (!(current.node instanceof Element)) continue;
    const tag = current.node.tagName.toLowerCase();
    if (DROP_WITH_CONTENT.has(tag)) continue;
    const safe = createSafeElement(current.node, output);
    const parent = safe ?? current.parent;
    if (safe) current.parent.appendChild(safe);
    for (let index = current.node.childNodes.length - 1; index >= 0; index -= 1) {
      const child = current.node.childNodes.item(index);
      if (child) pending.push({ node: child, parent, depth: current.depth + 1 });
    }
  }
  return `<!doctype html>${output.documentElement.outerHTML}`;
}

export function ProtocolSafeDisplay({ html }: { html: string }) {
  const [state, setState] = useState<
    | { kind: "loading" }
    | { kind: "too-large"; source: string }
    | { kind: "too-complex"; source: string }
    | { kind: "ready"; source: string; srcDoc: string }
  >({ kind: "loading" });

  useEffect(() => {
    // DOMParser 和 DOM Node 只在挂载后使用，SSR 不生成不同的 iframe 内容。
    const task = window.setTimeout(() => {
      if (displayHtmlIsTooLarge(html)) setState({ kind: "too-large", source: html });
      else {
        try {
          const srcDoc = sanitizeDisplay(html);
          setState(srcDoc
            ? { kind: "ready", source: html, srcDoc }
            : { kind: "too-complex", source: html });
        } catch {
          setState({ kind: "too-complex", source: html });
        }
      }
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
  if (state.kind === "too-complex") {
    return (
      <Alert status="warning">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>协议视图结构过于复杂，已禁止渲染</Alert.Title>
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
      title="协议包安全展示"
      srcDoc={state.srcDoc}
    />
  );
}

export const SocketSafeDisplay = ProtocolSafeDisplay;
