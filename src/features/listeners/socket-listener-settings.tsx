"use client";

import { useState, type ReactNode } from "react";
import { Alert, Card, Label, ListBox, NumberField, Select } from "@heroui/react";
import type {
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import {
  bindPackage,
  matchingOption,
  setSocketTopology,
} from "./socket-listener-model";
import { SocketProcessingCard, type ProtocolCatalogState } from "./socket-processing-card";
import { SocketAppSecurityCard, SocketServerCard } from "./socket-security-cards";

type Props = {
  settings: SocketRelaySettings;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  protocolCatalog: ProtocolCatalogState;
  locked: boolean;
  fieldErrors?: Record<string, string[]>;
  busy: boolean;
  testing: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
  onChange: (changes: Partial<SocketRelaySettings>) => void;
  onImportDownstreamServerIdentity: (label: string, password: string) => Promise<boolean>;
  onImportDownstreamClientTrust: (label: string) => Promise<boolean>;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTest: () => Promise<void>;
};

export function SocketListenerSettings(props: Props): ReactNode {
  const [announcement, setAnnouncement] = useState("");
  const apply = (settings: SocketRelaySettings) => props.onChange(settings);
  const topologyMode = props.settings.topology.mode;
  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      {props.locked && (
        <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>运行快照已锁定</Alert.Title>
          <Alert.Description>停止当前入口后才能修改工作方式、地址、证书和协议包。</Alert.Description>
        </Alert.Content></Alert>
      )}
      <SocketFieldErrors errors={props.fieldErrors} />
      <Card>
        <Card.Header><Card.Title>1. 响应方式</Card.Title>
          <Card.Description>选择把数据转发到上游服务，还是由本机直接生成应答。</Card.Description>
        </Card.Header>
        <Card.Content className="grid gap-4 md:grid-cols-2">
          <Select aria-label="Socket 响应方式" selectedKey={topologyMode} isDisabled={props.locked}
            onSelectionChange={(key) => {
              if (key !== "relay" && key !== "local_responder") return;
              let next = setSocketTopology(
                props.settings,
                key,
                props.protocolCatalog.data?.recommended_package,
              );
              if (next.processing.mode === "scripted") {
                const option = matchingOption(props.protocolCatalog.data, next.processing.settings.package);
                if (option) next = { ...next, processing: bindPackage(next.processing, option, key === "local_responder") };
              }
              apply(next);
              setAnnouncement(key === "relay"
                ? "已切换为转发到上游；不选择协议包时将透明转发。"
                : "已切换为本机应答；Server 地址和安全设置已清除。");
            }}>
            <Label>响应方式</Label><Select.Trigger className="h-10 min-h-10"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              <ListBox.Item id="relay" textValue="转发到上游">转发到上游</ListBox.Item>
              <ListBox.Item id="local_responder" textValue="本机应答">本机应答</ListBox.Item>
            </ListBox></Select.Popover>
          </Select>
          <NumberField aria-label="Socket 最大并发连接" minValue={1} maxValue={5000}
            value={props.settings.maximum_connections} isDisabled={props.locked}
            onChange={(maximum_connections) => props.onChange({ maximum_connections })}>
            <Label>最大并发连接</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
          </NumberField>
        </Card.Content>
      </Card>
      {announcement && <p role="status" aria-live="polite" className="text-sm text-[var(--telemetry-muted)]">
        {announcement}
      </p>}
      {/* 锁状态变化时重建证书卡，可立即关闭已打开的导入 Dialog，防止运行快照锁被绕过。 */}
      <SocketAppSecurityCard key={`app-security-${props.locked}`} settings={props.settings} certificateReferences={props.certificateReferences}
        certificateDetails={props.certificateDetails} locked={props.locked} busy={props.busy}
        onChange={apply} onImportIdentity={props.onImportDownstreamServerIdentity}
        onImportTrust={props.onImportDownstreamClientTrust} />
      {topologyMode === "relay" && <SocketServerCard key={`server-security-${props.locked}`} settings={props.settings} certificateReferences={props.certificateReferences}
        certificateDetails={props.certificateDetails} locked={props.locked} busy={props.busy}
        testing={props.testing} testResult={props.testResult} testError={props.testError}
        onChange={apply} onImportIdentity={props.onImportClientIdentity}
        onImportTrust={props.onImportServerTrust} onTest={props.onTest} />}
      <SocketProcessingCard settings={props.settings} catalog={props.protocolCatalog}
        locked={props.locked} onChange={apply} />
    </div>
  );
}

/**
 * Rust 返回稳定字段路径；这里按路径把消息归到用户实际操作的类别中，
 * 不向主流程暴露实现字段名，也不从自然语言猜测修复动作。
 */
function SocketFieldErrors({ errors }: { errors?: Record<string, string[]> }): ReactNode {
  const groups = [
    { label: "Socket 连接", tokens: ["topology", "downstream_security"] },
    // 规则路径也可能含 package/schema，必须先归入内容处理规则而不是包绑定。
    { label: "内容处理规则", tokens: ["protocol_rules", "rule"] },
    { label: "精确协议包", tokens: ["package", "schema"] },
    { label: "协议处理", tokens: ["capability", "processing"] },
  ];
  const entries = Object.entries(errors ?? {});
  const matched = new Set<string>();
  const cards = groups.flatMap((group) => {
    const fields = entries.filter(([path]) => !matched.has(path)
      && group.tokens.some((token) => path.toLowerCase().includes(token)));
    fields.forEach(([path]) => matched.add(path));
    return fields.length > 0 ? [{ ...group, fields }] : [];
  });
  const remaining = entries.filter(([path]) => path.includes("data_plane.settings") && !matched.has(path));
  if (remaining.length > 0) cards.push({ label: "Socket 配置", tokens: [], fields: remaining });
  return <>{cards.map((group) => <Alert key={group.label} status="danger">
    <Alert.Indicator /><Alert.Content><Alert.Title>{group.label}需要修正</Alert.Title>
      <Alert.Description>
        <p>保存前请修正这部分配置。</p>
        <details className="mt-2">
          <summary className="cursor-pointer font-medium">高级诊断</summary>
          <ul className="mt-1 space-y-1">
            {group.fields.map(([path, messages]) => <li key={path}>{path}: {messages.join("，")}</li>)}
          </ul>
        </details>
      </Alert.Description>
    </Alert.Content>
  </Alert>)}</>;
}
