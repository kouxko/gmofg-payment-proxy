"use client";

import { useMemo } from "react";
import { formatBytes } from "@/lib/format";

function hexLines(bytes: number[]): string[] {
  const lines: string[] = [];
  for (let index = 0; index < bytes.length; index += 16) {
    const row = bytes.slice(index, index + 16);
    const hex = row.map((value) => value.toString(16).padStart(2, "0")).join(" ").padEnd(47);
    const ascii = row.map((value) => value >= 32 && value <= 126 ? String.fromCharCode(value) : ".").join("");
    lines.push(`${index.toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  return lines;
}

/** 将本次读写的完整 Socket 字节直接呈现为 Hex + ASCII，不增加分页状态。 */
export function SocketByteViewer({ bytes, label }: { bytes: number[]; label: string }) {
  const lines = useMemo(() => hexLines(bytes), [bytes]);
  return (
    <section aria-label={label} className="space-y-3">
      <p className="text-xs text-[var(--telemetry-muted)]">{formatBytes(bytes.length)}</p>
      {bytes.length === 0 ? (
        <p className="rounded-xl border border-[var(--telemetry-line)] p-6 text-center text-sm">空字节流（0 B）</p>
      ) : (
        <pre
          aria-label={label}
          className="max-h-[55vh] overflow-auto rounded-xl bg-[var(--telemetry-panel)] p-4 font-mono text-xs leading-5"
          role="region"
          tabIndex={0}
        >
          {lines.join("\n")}
        </pre>
      )}
    </section>
  );
}
