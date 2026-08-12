"use client";

import { Label, ListBox, Select } from "@heroui/react";
import type { BodyCodecKind } from "@/generated/rust-types";

const codecOptions: Array<{ id: BodyCodecKind; label: string }> = [
  { id: "auto", label: "自动（读取 Content-Type charset）" },
  { id: "utf8", label: "强制 UTF-8" },
  { id: "shift_jis", label: "强制 Shift-JIS" },
  { id: "raw", label: "原始字节（不解码）" },
];

type Props = {
  requestCodec: BodyCodecKind;
  responseCodec: BodyCodecKind;
  onRequestCodecChange: (codec: BodyCodecKind) => void;
  onResponseCodecChange: (codec: BodyCodecKind) => void;
};

export function BodyCodecSettings({
  requestCodec,
  responseCodec,
  onRequestCodecChange,
  onResponseCodecChange,
}: Props) {
  return (
    <section
      aria-label="HTTP 正文编码"
      className={[
        "col-span-2 grid min-w-0 grid-cols-[minmax(16rem,1fr)_minmax(13rem,16rem)_minmax(13rem,16rem)]",
        "items-end gap-3 rounded-xl border border-[var(--telemetry-line)]",
        "bg-[var(--telemetry-table-head)]/45 px-4 py-3",
        "max-[900px]:grid-cols-2 max-[700px]:col-span-1 max-[700px]:grid-cols-1",
      ].join(" ")}
    >
      <div className="min-w-0 self-center max-[900px]:col-span-2 max-[700px]:col-span-1">
        <p className="text-sm font-semibold">HTTP 正文编码</p>
        <p className="mt-0.5 text-xs text-[var(--telemetry-muted)]">
          自动模式遵循 Header；强制模式覆盖 charset。未修改正文始终保持原始字节透传。
        </p>
      </div>
      <CodecSelect
        label="请求正文编码"
        value={requestCodec}
        onChange={onRequestCodecChange}
      />
      <CodecSelect
        label="响应正文编码"
        value={responseCodec}
        onChange={onResponseCodecChange}
      />
    </section>
  );
}

function CodecSelect({
  label,
  value,
  onChange,
}: {
  label: string;
  value: BodyCodecKind;
  onChange: (value: BodyCodecKind) => void;
}) {
  return (
    <Select
      aria-label={label}
      selectedKey={value}
      onSelectionChange={(key) => onChange(String(key) as BodyCodecKind)}
    >
      <Label>{label}</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          {codecOptions.map((option) => (
            <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
              {option.label}
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}
