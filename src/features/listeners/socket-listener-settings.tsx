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
  setProcessingMode,
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
  onImportDownstreamServerIdentity: (label: string) => Promise<boolean>;
  onImportDownstreamClientTrust: (label: string) => Promise<boolean>;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTest: () => Promise<void>;
};

export function SocketListenerSettings(props: Props): ReactNode {
  const [announcement, setAnnouncement] = useState("");
  const apply = (settings: SocketRelaySettings) => props.onChange(settings);
  // 旧 Workspace 可能尚未序列化 processing；界面按向后兼容的 Direct 解释。
  const processingMode = props.settings.processing?.mode ?? "direct";
  const topologyMode = props.settings.topology.mode;
  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      {props.locked && (
        <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>运行快照已锁定</Alert.Title>
          <Alert.Description>停止当前 Listener 后才能修改模式、拓扑、地址、证书、协议包和处理开关。</Alert.Description>
        </Alert.Content></Alert>
      )}
      <SocketFieldErrors errors={props.fieldErrors} />
      <Card>
        <Card.Header><Card.Title>1. Socket 模式</Card.Title>
          <Card.Description>Direct 透明转发完整字节；Scripted 才加载精确协议包、Frame、Document 与规则。</Card.Description>
        </Card.Header>
        <Card.Content className="grid gap-4 md:grid-cols-3">
          <Select aria-label="Socket 数据处理模式" selectedKey={processingMode} isDisabled={props.locked}
            onSelectionChange={(key) => {
              if (key !== "direct" && key !== "scripted") return;
              apply(setProcessingMode(props.settings, key));
              setAnnouncement(key === "direct"
                ? "已切换 Direct；已恢复 Relay 并关闭脚本处理。"
                : "已切换 Scripted；请选择精确协议包并配置方向能力。");
            }}>
            <Label>数据处理</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox><ListBox.Item id="direct" textValue="Direct">Direct</ListBox.Item><ListBox.Item id="scripted" textValue="Scripted">Scripted</ListBox.Item></ListBox></Select.Popover>
          </Select>
          <Select aria-label="Socket 连接拓扑" selectedKey={topologyMode} isDisabled={props.locked}
            onSelectionChange={(key) => {
              if (key !== "relay" && key !== "local_responder") return;
              apply(setSocketTopology(props.settings, key));
              setAnnouncement(key === "local_responder"
                ? "已切换 LocalResponder；已移除远端目标并启用 Scripted。"
                : "已切换 Relay；请配置远端目标。");
            }}>
            <Label>连接拓扑</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox><ListBox.Item id="relay" textValue="Relay">Relay</ListBox.Item><ListBox.Item id="local_responder" textValue="LocalResponder">LocalResponder</ListBox.Item></ListBox></Select.Popover>
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
      {processingMode === "direct" ? (
        <Card><Card.Header><Card.Title>4. Direct 字节转发</Card.Title>
          <Card.Description>不加载 Rhai、不切分 Frame、不生成 Document，也不执行 Socket Document 规则或 Display。</Card.Description>
        </Card.Header></Card>
      ) : (
        <SocketProcessingCard settings={props.settings} catalog={props.protocolCatalog}
          locked={props.locked} onChange={apply} />
      )}
    </div>
  );
}

/**
 * Rust 返回稳定字段路径；这里保留原路径和消息，同时把错误放到用户实际操作的
 * 拓扑、精确包、方向能力或规则类别中，不从自然语言猜测修复动作。
 */
function SocketFieldErrors({ errors }: { errors?: Record<string, string[]> }): ReactNode {
  const groups = [
    { label: "Socket 拓扑", tokens: ["topology", "downstream_security"] },
    // 规则路径也可能含 package/schema，必须先归入 Document 规则而不是包绑定。
    { label: "Document 规则", tokens: ["socket_rules", "rule"] },
    { label: "精确协议包", tokens: ["package", "schema"] },
    { label: "方向能力", tokens: ["capability", "decode_enabled", "encode_enabled", "processing"] },
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
      <Alert.Description>{group.fields.map(([path, messages]) => `${path}: ${messages.join("，")}`).join("；")}</Alert.Description>
    </Alert.Content>
  </Alert>)}</>;
}
