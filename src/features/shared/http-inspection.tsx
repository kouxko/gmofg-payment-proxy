"use client";

import type { ReactNode } from "react";
import { Chip, FieldError, TextArea, TextField } from "@heroui/react";
import {
  formatMessageBody,
  messageCodecLabel,
  messageCharset,
  messageContentKind,
  messageContentLabel,
  messageMediaType,
  requestQueryString,
  type InspectableMessage,
} from "@/lib/message-content";

type BodyViewerProps = {
  label: string;
  message?: InspectableMessage | null;
  emptyText: string;
  textOverride?: string;
  editable?: boolean;
  error?: string;
  ariaLabel?: string;
  showRawBytes?: boolean;
  onChange?: (value: string) => void;
};

const MAX_TEXT_PREVIEW_CHARS = 256 * 1024;
const MAX_TEXT_PREVIEW_LINES = 5_000;
const MAX_BYTE_PREVIEW_BYTES = 64 * 1024;

export function HttpBodyViewer({
  label,
  message,
  emptyText,
  textOverride,
  editable = false,
  error,
  ariaLabel = label,
  showRawBytes = true,
  onChange,
}: BodyViewerProps) {
  const body = textOverride ?? formatMessageBody(message, emptyText);
  const kind = messageContentKind(message);
  const presentation = editable
    ? { text: body, kind, detectedLabel: undefined }
    : formatBodyForPresentation(message, body, kind);
  const bytes = message?.body_bytes ?? [];
  const textPreview = editable
    ? { value: presentation.text, truncated: false }
    : previewText(presentation.text);
  const bytePreview = bytes.slice(0, MAX_BYTE_PREVIEW_BYTES);
  const bytesTruncated = bytePreview.length < bytes.length;

  return (
    <section className="min-w-0 space-y-2" aria-label={label}>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <Chip size="sm" variant="soft">{messageContentLabel(message)}</Chip>
        {messageMediaType(message) && (
          <Chip size="sm" variant="soft">{messageMediaType(message)}</Chip>
        )}
        {messageCharset(message) && (
          <Chip size="sm" variant="soft">charset={messageCharset(message)}</Chip>
        )}
        {message?.codec_id && (
          <Chip size="sm" variant="soft">codec={messageCodecLabel(message.codec_id)}</Chip>
        )}
        {presentation.detectedLabel && (
          <Chip size="sm" color="accent" variant="soft">
            {presentation.detectedLabel}
          </Chip>
        )}
      </div>
      {message?.decode_error && (
        <p className="text-xs text-danger">{message.decode_error}</p>
      )}
      {(textPreview.truncated || bytesTruncated) && (
        <p className="text-xs text-warning">
          正文较大，仅渲染安全预览；完整原始数据仍保存在当前会话中。
        </p>
      )}
      {editable ? (
        <EditableCodeSurface
          ariaLabel={ariaLabel}
          value={textPreview.value}
          error={error}
          onChange={onChange}
        />
      ) : (
        <CodeSurface
          text={textPreview.value}
          kind={presentation.kind}
          ariaLabel={ariaLabel}
        />
      )}
      {showRawBytes && (
        <details className="group rounded-lg" open={kind === "binary"}>
          <summary className="cursor-pointer text-xs font-medium text-[var(--telemetry-muted)]">
            {bytesTruncated
              ? `原始字节（总计 ${bytes.length} bytes，预览 ${bytePreview.length} bytes）`
              : `原始字节（${bytes.length} bytes）`}
          </summary>
          <pre
            className={[
              "mt-2 max-h-48 overflow-auto rounded-lg border p-3",
              "border-[var(--telemetry-line)] bg-[var(--telemetry-table-head)]",
              "font-mono text-xs",
            ].join(" ")}
          >
            {formatBytes(bytePreview)}
          </pre>
        </details>
      )}
    </section>
  );
}

export function HttpRequestTargetView({
  method,
  target,
  queryString,
}: {
  method: string;
  target: string;
  queryString?: string | null;
}) {
  const query = requestQueryString({ query_string: queryString }, target);
  return (
    <div className="min-w-0 space-y-2" aria-label="HTTP 请求目标">
      <div
        className={[
          "flex min-w-0 items-start gap-2 rounded-lg border p-3",
          "border-[var(--telemetry-line)] bg-[var(--telemetry-table-head)]",
          "font-mono text-xs",
        ].join(" ")}
      >
        <Chip size="sm" variant="soft">{method}</Chip>
        <span className="min-w-0 break-all">{target}</span>
      </div>
      {query !== undefined && (
        <div className="grid grid-cols-[max-content_minmax(0,1fr)] gap-2 text-xs">
          <span className="text-[var(--telemetry-muted)]">Query</span>
          <code className="break-all">{query || "（空 query）"}</code>
        </div>
      )}
    </div>
  );
}

function EditableCodeSurface({
  ariaLabel,
  value,
  error,
  onChange,
}: {
  ariaLabel: string;
  value: string;
  error?: string;
  onChange?: (value: string) => void;
}) {
  return (
    <TextField aria-label={`${ariaLabel}字段`} isInvalid={Boolean(error)}>
      <div
        className={[
          "grid min-w-0 w-full max-w-full grid-cols-[3rem_minmax(0,1fr)] overflow-hidden rounded-lg border",
          "border-[var(--telemetry-line)] bg-[var(--telemetry-table-head)]",
        ].join(" ")}
      >
        <LineNumberGutter count={lineCount(value)} />
        <TextArea
          aria-label={ariaLabel}
          className="min-h-[320px] max-h-[520px] min-w-0 resize-none overflow-auto border-0 font-mono text-xs"
          value={value}
          onChange={(event) => onChange?.(event.target.value)}
        />
      </div>
      {error && <FieldError>{error}</FieldError>}
    </TextField>
  );
}

function CodeSurface({
  text,
  kind,
  ariaLabel,
}: {
  text: string;
  kind: ReturnType<typeof messageContentKind>;
  ariaLabel: string;
}) {
  const lines = text.split("\n");
  return (
    <div
      aria-label={ariaLabel}
      data-code-surface={kind}
      className={[
        "min-h-[320px] max-h-[520px] min-w-0 w-full max-w-full overflow-auto rounded-lg border",
        "border-[var(--telemetry-line)] bg-[var(--telemetry-table-head)]",
      ].join(" ")}
    >
      {lines.map((line, index) => (
        <div
          key={index}
          data-code-row
          className="grid min-w-0 w-full grid-cols-[3rem_minmax(0,1fr)] font-mono text-xs leading-5"
        >
          <span
            data-line-number
            className={[
              "select-none border-r border-[var(--telemetry-line)] px-2",
              "text-right text-[var(--telemetry-muted)]",
            ].join(" ")}
          >
            {index + 1}
          </span>
          <code className="min-h-5 min-w-0 whitespace-pre-wrap break-all px-3">
            {highlightLine(line, kind)}
          </code>
        </div>
      ))}
    </div>
  );
}

function formatBodyForPresentation(
  message: InspectableMessage | null | undefined,
  body: string,
  declaredKind: ReturnType<typeof messageContentKind>,
): {
  text: string;
  kind: ReturnType<typeof messageContentKind>;
  detectedLabel?: string;
} {
  if (message?.json != null) return { text: body, kind: "json" };

  const source = message?.body_text?.trim();
  if (!source) return { text: body, kind: declaredKind };

  if (source.startsWith("{") || source.startsWith("[")) {
    try {
      return {
        text: JSON.stringify(JSON.parse(source), null, 2),
        kind: "json",
        detectedLabel: declaredKind === "json" ? undefined : "格式化为 JSON 展示",
      };
    } catch {
      // Invalid JSON remains byte-faithful decoded text in the viewer.
    }
  }

  if (declaredKind === "xml") {
    return { text: formatXmlForPresentation(source), kind: "xml" };
  }

  return { text: body, kind: declaredKind };
}

function formatXmlForPresentation(xml: string) {
  return xml
    .replace(/>\s*</g, ">\n<")
    .split("\n")
    .reduce<{ depth: number; lines: string[] }>((state, line) => {
      const closes = /^<\//.test(line);
      const depth = Math.max(0, state.depth - Number(closes));
      state.lines.push(`${"  ".repeat(depth)}${line}`);
      const opens = /^<[^!?/][^>]*[^/]?>$/.test(line) && !/<\/[^>]+>$/.test(line);
      state.depth = depth + Number(opens);
      return state;
    }, { depth: 0, lines: [] })
    .lines.join("\n");
}

function LineNumberGutter({ count }: { count: number }) {
  return (
    <pre
      className={[
        "select-none border-r border-[var(--telemetry-line)] px-2 py-2",
        "text-right font-mono text-xs leading-5 text-[var(--telemetry-muted)]",
      ].join(" ")}
    >
      {Array.from({ length: count }, (_, index) => index + 1).join("\n")}
    </pre>
  );
}

function highlightLine(
  line: string,
  kind: ReturnType<typeof messageContentKind>,
) {
  if (kind === "json") {
    return tokenizedLine(line, JSON_TOKEN, (token, index) =>
      line.slice(index + token.length).trimStart().startsWith(":")
        ? "json-key"
        : jsonTokenType(token),
    );
  }
  if (kind === "xml") return tokenizedLine(line, XML_TOKEN, () => "xml-tag");
  return line;
}

const JSON_TOKEN =
  /"(?:\\.|[^"\\])*"(?=\s*:)|"(?:\\.|[^"\\])*"|\b(?:true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g;
const XML_TOKEN = /<!--[\s\S]*?-->|<\/?[A-Za-z_][^>]*>|&(?:#\d+|#x[\da-f]+|[A-Za-z][\w.-]*);/gi;

function tokenizedLine(
  line: string,
  pattern: RegExp,
  tokenType: (token: string, index: number) => string,
): ReactNode[] {
  const parts: ReactNode[] = [];
  let cursor = 0;
  for (const match of line.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > cursor) parts.push(line.slice(cursor, index));
    parts.push(
      <span
        key={`${index}-${match[0]}`}
        data-token={tokenType(match[0], index)}
        className="text-[var(--telemetry-accent)]"
      >
        {match[0]}
      </span>,
    );
    cursor = index + match[0].length;
  }
  if (cursor < line.length) parts.push(line.slice(cursor));
  return parts;
}

function jsonTokenType(token: string) {
  if (token.startsWith('"')) return "json-string";
  if (token === "true" || token === "false" || token === "null") return "json-literal";
  return "json-number";
}

function lineCount(value: string) {
  return Math.max(1, value.split("\n").length);
}

function formatBytes(bytes: number[]) {
  if (bytes.length === 0) return "无原始字节";
  return bytes
    .reduce<string[]>((lines, byte, index) => {
      const line = Math.floor(index / 16);
      lines[line] = `${lines[line] ?? ""}${byte.toString(16).padStart(2, "0")}${index % 16 === 15 ? "" : " "}`;
      return lines;
    }, [])
    .join("\n");
}

function previewText(text: string) {
  let end = Math.min(text.length, MAX_TEXT_PREVIEW_CHARS);
  let lines = 1;
  for (let index = 0; index < end; index += 1) {
    if (text.charCodeAt(index) !== 10) continue;
    lines += 1;
    if (lines > MAX_TEXT_PREVIEW_LINES) {
      end = index;
      break;
    }
  }
  return { value: text.slice(0, end), truncated: end < text.length };
}
