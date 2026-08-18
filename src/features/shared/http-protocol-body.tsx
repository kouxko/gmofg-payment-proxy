"use client";

import { Alert, Chip, Tabs } from "@heroui/react";
import type {
  Document,
  HttpProtocolDisplayFallbackReason,
  HttpProtocolRuleStageViewModel,
  MessageContentViewModel,
  ProtocolRuleStage,
} from "@/generated/rust-types";
import { ProtocolSafeDisplay } from "@/features/capture/socket-safe-display";
import { HttpBodyViewer } from "./http-inspection";

const DISPLAY_FALLBACK_TEXT: Record<HttpProtocolDisplayFallbackReason, string> = {
  entry_point_failed: "协议视图生成失败，请查看原始或写出 Body。",
  resource_limit_exceeded: "协议视图超过处理限制，请查看原始或写出 Body。",
};

function DocumentView({ document }: { document: Document }) {
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
                  <code className="text-xs font-normal text-[var(--telemetry-muted)]">
                    {field.name} · {field.type}
                  </code>
                </th>
                <td className="min-w-0 break-all px-3 py-2 align-top">
                  {value === null ? (
                    <span className="text-[var(--telemetry-muted)]">未设置</span>
                  ) : value.type === "blob" ? (
                    <code className="text-xs">{value.value.map((byte) => byte.toString(16).padStart(2, "0")).join(" ")}</code>
                  ) : value.type === "bool" ? (
                    String(value.value)
                  ) : (
                    <code className="text-xs">{value.value}</code>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function ProtocolView({
  document,
  display,
}: Pick<HttpProtocolRuleStageViewModel, "document" | "display">) {
  if (display.kind === "untrusted_html") {
    return <ProtocolSafeDisplay html={display.html} />;
  }
  return (
    <div className="space-y-3">
      <Alert status="warning">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>协议视图生成失败</Alert.Title>
          <Alert.Description>{DISPLAY_FALLBACK_TEXT[display.reason]}</Alert.Description>
        </Alert.Content>
      </Alert>
      <DocumentView document={document} />
    </div>
  );
}

function ProtocolBodyText({ label, text }: { label: string; text: string }) {
  return (
    <HttpBodyViewer
      label={label}
      message={null}
      emptyText="Body 为空"
      textOverride={text}
      showRawBytes={false}
    />
  );
}

const STAGE_LABELS: Record<ProtocolRuleStage, string> = {
  app_to_proxy: "应用 → 代理",
  proxy_to_upstream: "代理 → 上游服务",
  upstream_to_proxy: "上游服务 → 代理",
  proxy_to_app: "代理 → 应用",
};

export function HttpProtocolBodyViewer({
  label,
  message,
  emptyText,
}: {
  label: string;
  message?: MessageContentViewModel | null;
  emptyText: string;
}) {
  if (message?.protocol_failure) {
    const failure = message.protocol_failure;
    return (
      <section className="space-y-3" aria-label={`${label}协议处理失败`}>
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>协议处理失败</Alert.Title>
            <Alert.Description>
              {failure.detail} · <code>{failure.code}</code>
              {failure.stage ? ` · ${STAGE_LABELS[failure.stage]}` : ""}
            </Alert.Description>
          </Alert.Content>
        </Alert>
        <HttpBodyViewer label={label} message={message} emptyText={emptyText} />
      </section>
    );
  }
  if (!message?.protocol) {
    return <HttpBodyViewer label={label} message={message} emptyText={emptyText} />;
  }
  const protocol = message.protocol;
  const matched = protocol.stages.reduce(
    (count, stage) => count + stage.matched_rule_ids.length,
    0,
  );
  return (
    <section className="space-y-3" aria-label={`${label}协议处理结果`}>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <Chip size="sm" variant="soft">
          {protocol.package.id}@{protocol.package.version}
        </Chip>
        <Chip size="sm" variant="soft">{protocol.document.schema.title}</Chip>
        <Chip size="sm" color={matched > 0 ? "accent" : "default"} variant="soft">
          命中 {matched} 条规则
        </Chip>
      </div>
      <Tabs defaultSelectedKey="final">
        <Tabs.ListContainer>
          <Tabs.List aria-label={`${label}查看方式`}>
            {protocol.stages.map((stage) => (
              <Tabs.Tab id={stage.stage} key={stage.stage}>
                {STAGE_LABELS[stage.stage]}<Tabs.Indicator />
              </Tabs.Tab>
            ))}
            <Tabs.Tab id="final">最终协议视图<Tabs.Indicator /></Tabs.Tab>
            <Tabs.Tab id="origin">原始 Body<Tabs.Indicator /></Tabs.Tab>
            <Tabs.Tab id="written">写出 Body<Tabs.Indicator /></Tabs.Tab>
          </Tabs.List>
        </Tabs.ListContainer>
        {protocol.stages.map((stage) => (
          <Tabs.Panel id={stage.stage} className="pt-4" key={stage.stage}>
            <ProtocolView document={stage.document} display={stage.display} />
          </Tabs.Panel>
        ))}
        <Tabs.Panel id="final" className="pt-4">
          <ProtocolView document={protocol.document} display={protocol.display} />
        </Tabs.Panel>
        <Tabs.Panel id="origin" className="pt-4">
          <ProtocolBodyText label={`${label}原始 Body`} text={protocol.origin_text} />
        </Tabs.Panel>
        <Tabs.Panel id="written" className="pt-4">
          <ProtocolBodyText label={`${label}写出 Body`} text={protocol.written_text} />
        </Tabs.Panel>
      </Tabs>
    </section>
  );
}
