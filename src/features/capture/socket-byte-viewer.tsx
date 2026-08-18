"use client";

import { useMemo, useState } from "react";
import { Button, Tabs } from "@heroui/react";
import type {
  SocketCaptureDocument,
  SocketDisplayResult,
} from "@/generated/rust-types";
import { formatBytes } from "@/lib/format";
import { displayHtmlIsTooLarge, SocketSafeDisplay } from "./socket-safe-display";

const HEX_PAGE_BYTES = 4096;

function hexLines(bytes: number[], offset: number): string[] {
  const lines: string[] = [];
  for (let index = 0; index < bytes.length; index += 16) {
    const row = bytes.slice(index, index + 16);
    const hex = row.map((value) => value.toString(16).padStart(2, "0")).join(" ").padEnd(47);
    const ascii = row.map((value) => value >= 32 && value <= 126 ? String.fromCharCode(value) : ".").join("");
    lines.push(`${(offset + index).toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  return lines;
}

/** 4 KiB 分页仅限制当前 DOM；上一页/下一页可到达完整字节，不代表数据被截断。 */
export function PaginatedHex({ bytes, label }: { bytes: number[]; label: string }) {
  const [page, setPage] = useState(1);
  const totalPages = Math.max(1, Math.ceil(bytes.length / HEX_PAGE_BYTES));
  const safePage = Math.min(page, totalPages);
  const start = (safePage - 1) * HEX_PAGE_BYTES;
  const lines = useMemo(() => hexLines(bytes.slice(start, start + HEX_PAGE_BYTES), start), [bytes, start]);
  return (
    <section aria-label={label} className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--telemetry-muted)]">
        <span>{formatBytes(bytes.length)}</span>
        <span>·</span>
        <span>界面按 4 KiB 分页，原始数据未截断</span>
      </div>
      {bytes.length === 0 ? (
        <p className="rounded-xl border border-[var(--telemetry-line)] p-6 text-center text-sm">空字节流（0 B）</p>
      ) : (
        <pre
          aria-label={`${label}，字节页 ${safePage}/${totalPages}`}
          className="max-h-[55vh] overflow-auto rounded-xl bg-[var(--telemetry-panel)] p-4 font-mono text-xs leading-5"
          role="region"
          tabIndex={0}
        >
          {lines.join("\n")}
        </pre>
      )}
      <div className="flex items-center justify-end gap-2">
        <Button size="sm" variant="outline" isDisabled={safePage <= 1} onPress={() => setPage((value) => Math.max(1, value - 1))}>上一字节页</Button>
        <span className="text-sm">{safePage} / {totalPages}</span>
        <Button size="sm" variant="outline" isDisabled={safePage >= totalPages} onPress={() => setPage((value) => Math.min(totalPages, value + 1))}>下一字节页</Button>
      </div>
    </section>
  );
}

export function SocketDocumentView({ document }: { document: SocketCaptureDocument }) {
  return (
    <div className="overflow-hidden rounded-xl border border-[var(--telemetry-line)]">
      <table className="w-full table-fixed border-collapse text-sm">
        <tbody>
          {document.schema.fields.map((field, index) => {
            const value = document.values[index];
            return (
              <tr className="border-b border-[var(--telemetry-line)] last:border-b-0" key={field.name}>
                <th
                  className="w-1/2 border-r border-[var(--telemetry-line)] bg-[var(--telemetry-panel)] px-3 py-2 text-left align-top font-medium"
                  scope="row"
                >
                  <span>{field.label}</span><br />
                  <code className="text-xs font-normal text-[var(--telemetry-muted)]">{field.name} · {field.type}</code>
                </th>
                <td className="min-w-0 break-all px-3 py-2 align-top">
                  {value === null ? <span className="text-[var(--telemetry-muted)]">未设置</span>
                    : value.type === "blob" ? <PaginatedHex bytes={value.value} label={`${field.label} Blob 字节`} />
                      : value.type === "bool" ? String(value.value)
                        : <code className="text-xs">{value.value}</code>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

interface ProtocolHexViewerProps {
  bytes: number[];
  document?: SocketCaptureDocument | null;
  display?: SocketDisplayResult;
  label: string;
  decodeDisabled?: boolean;
  preferDocument?: boolean;
}

export function ProtocolHexViewer(props: ProtocolHexViewerProps) {
  const htmlDisplay = props.display?.type === "untrusted_html" ? props.display : undefined;
  const hasHtml = Boolean(htmlDisplay);
  const htmlTooLarge = htmlDisplay ? displayHtmlIsTooLarge(htmlDisplay.html) : false;
  // 自定义协议视图只有成功且未超限才默认展示；本机请求可显式让内置字段视图
  // 默认显示。协议视图降级时即使仍有字段报文，也必须默认 Hex。
  const defaultProtocol = (hasHtml && !htmlTooLarge) || (props.preferDocument && Boolean(props.document));
  const [tab, setTab] = useState<"protocol" | "hex">(defaultProtocol ? "protocol" : "hex");
  return (
    <Tabs selectedKey={tab} onSelectionChange={(key) => setTab(key as "protocol" | "hex")}>
      <Tabs.ListContainer>
        <Tabs.List aria-label={`${props.label}查看方式`}>
          <Tabs.Tab id="protocol">协议视图<Tabs.Indicator /></Tabs.Tab>
          <Tabs.Tab id="hex">Hex<Tabs.Indicator /></Tabs.Tab>
        </Tabs.List>
      </Tabs.ListContainer>
      <Tabs.Panel id={tab} className="pt-4">
        {tab === "hex" ? <PaginatedHex bytes={props.bytes} label={`${props.label} Hex`} />
          : htmlDisplay ? <SocketSafeDisplay html={htmlDisplay.html} />
            : props.document ? <SocketDocumentView document={props.document} />
              : <p className="py-8 text-center text-sm text-[var(--telemetry-muted)]">
                {props.decodeDisabled ? "未解析出报文字段；请切换 Hex 查看完整字节。" : "协议视图不可用；请切换 Hex 查看完整字节。"}
              </p>}
      </Tabs.Panel>
    </Tabs>
  );
}
