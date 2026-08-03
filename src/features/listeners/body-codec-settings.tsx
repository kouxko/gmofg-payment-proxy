"use client";

import { Card, Label, ListBox, Select } from "@heroui/react";
import type { BodyCodecKind } from "@/generated/rust-types";

const codecLabels: Record<BodyCodecKind, string> = {
  raw: "原始字节（不解析）",
  utf8: "UTF-8",
  shift_jis: "Shift-JIS",
};

type Props = {
  requestCodec: BodyCodecKind;
  responseCodec: BodyCodecKind;
  onRequestCodecChange: (codec: BodyCodecKind) => void;
  onResponseCodecChange: (codec: BodyCodecKind) => void;
};

/**
 * 监听器级正文编码配置。
 *
 * 编码直接属于当前监听器，不再通过 Workspace 策略 ID 间接引用。前端只提交枚举值；
 * 解码、修改后的重新编码和 Content-Length 计算仍全部由 Rust 完成。
 */
export function BodyCodecSettings({
  requestCodec,
  responseCodec,
  onRequestCodecChange,
  onResponseCodecChange,
}: Props) {
  return (
    <Card className="col-span-2 max-[700px]:col-span-1">
      <Card.Header>
        <Card.Title>HTTP 正文编码</Card.Title>
        <Card.Description>
          指定当前监听器解析请求和响应正文时使用的编码。未修改的正文保持原始字节透传。
        </Card.Description>
      </Card.Header>
      <Card.Content className="grid grid-cols-2 gap-4 max-[700px]:grid-cols-1">
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
      </Card.Content>
    </Card>
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
    <div className="grid gap-2">
      <Select
        aria-label={label}
        selectedKey={value}
        onSelectionChange={(key) => onChange(String(key) as BodyCodecKind)}
      >
        <Label>{label}</Label>
        <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
        <Select.Popover>
          <ListBox>
            {Object.entries(codecLabels).map(([id, labelText]) => (
              <ListBox.Item key={id} id={id} textValue={labelText}>
                {labelText}
              </ListBox.Item>
            ))}
          </ListBox>
        </Select.Popover>
      </Select>
      <p className="text-xs text-[var(--telemetry-muted)]">
        {codecDescription(value)}
      </p>
    </div>
  );
}

function codecDescription(codec: BodyCodecKind) {
  if (codec === "raw") {
    return "不执行文本或 JSON 解码，适用于二进制或未知格式正文。";
  }
  if (codec === "shift_jis") {
    return "按 Shift-JIS 解码；正文被修改后由 Rust 重新编码并计算长度。";
  }
  return "按 UTF-8 解码；正文被修改后由 Rust 重新编码并计算长度。";
}
