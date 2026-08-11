"use client";

import { Card, Chip } from "@heroui/react";
import type { BodyCodecKind } from "@/generated/rust-types";

const codecLabels: Record<BodyCodecKind, string> = {
  auto: "按 Content-Type 自动识别",
  raw: "原始字节（不解析）",
  utf8: "UTF-8",
  shift_jis: "Shift-JIS",
};

type Props = {
  requestCodec: BodyCodecKind;
  responseCodec: BodyCodecKind;
};

/**
 * 正文编码由 Rust 按 Content-Type 自动识别。旧监听器的手动 codec 字段仍随草稿保存，
 * 仅用于兼容已有配置，不再提供会造成新旧识别策略冲突的编辑入口。
 */
export function BodyCodecSettings({
  requestCodec,
  responseCodec,
}: Props) {
  return (
    <Card className="col-span-2 max-[700px]:col-span-1">
      <Card.Header>
        <Card.Title>HTTP 正文识别</Card.Title>
        <Card.Description>
          按 Content-Type charset 自动识别。未修改的正文保持原始字节透传。
        </Card.Description>
      </Card.Header>
      <Card.Content className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-[var(--telemetry-muted)]">旧配置兼容值随保存保留：</span>
        <Chip size="sm" variant="soft">请求 {codecLabels[requestCodec]}</Chip>
        <Chip size="sm" variant="soft">响应 {codecLabels[responseCodec]}</Chip>
      </Card.Content>
    </Card>
  );
}
