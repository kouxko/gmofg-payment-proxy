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
  setSocketWorkingMode,
  socketWorkingMode,
  type SocketWorkingMode,
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
  const workingMode = socketWorkingMode(props.settings);
  const topologyMode = props.settings.topology.mode;
  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      {props.locked && (
        <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>运行快照已锁定</Alert.Title>
          <Alert.Description>停止当前 Listener 后才能修改工作方式、地址、证书、协议包和处理开关。</Alert.Description>
        </Alert.Content></Alert>
      )}
      <SocketFieldErrors errors={props.fieldErrors} />
      {workingMode === "incompatible" && <Alert status="danger"><Alert.Indicator /><Alert.Content>
        <Alert.Title>当前工作方式不兼容</Alert.Title>
        <Alert.Description>保存的数据组合无法安全解释，请重新选择一种工作方式。</Alert.Description>
      </Alert.Content></Alert>}
      <Card>
        <Card.Header><Card.Title>1. 工作方式</Card.Title>
          <Card.Description>选择数据是原样送到远端、按协议处理后送到远端，还是由本机直接返回结果。</Card.Description>
        </Card.Header>
        <Card.Content className="grid gap-4 md:grid-cols-2">
          <Select aria-label="Socket 工作方式" selectedKey={workingMode} isDisabled={props.locked}
            onSelectionChange={(key) => {
              if (!isSocketWorkingMode(key)) return;
              apply(setSocketWorkingMode(
                props.settings,
                key,
                props.protocolCatalog.data?.recommended_package,
              ));
              setAnnouncement(workingModeAnnouncement(key));
            }}>
            <Label>工作方式</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              <ListBox.Item id="raw_relay" textValue="透明转发">
                <WorkingModeOption title="透明转发" description="收到什么就转发什么，不解析也不修改" />
              </ListBox.Item>
              <ListBox.Item id="protocol_relay" textValue="按协议转发">
                <WorkingModeOption title="按协议转发" description="解析报文，可按规则修改，再转发到 Server" />
              </ListBox.Item>
              <ListBox.Item id="local_response" textValue="本地应答">
                <WorkingModeOption title="本地应答" description="不连接 Server，由本机解析请求并生成响应" />
              </ListBox.Item>
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
      {workingMode === "raw_relay" ? (
        <Card><Card.Header><Card.Title>4. 透明转发</Card.Title>
          <Card.Description>应用发送的数据保持原样送到远端，远端返回的数据也保持原样交给应用。</Card.Description>
        </Card.Header></Card>
      ) : (
        <SocketProcessingCard settings={props.settings} catalog={props.protocolCatalog}
          locked={props.locked} onChange={apply} />
      )}
    </div>
  );
}

function isSocketWorkingMode(value: React.Key | null): value is SocketWorkingMode {
  return value === "raw_relay" || value === "protocol_relay" || value === "local_response";
}

function WorkingModeOption({ title, description }: { title: string; description: string }): ReactNode {
  return <div className="grid gap-0.5"><span>{title}</span>
    <span className="text-xs text-[var(--telemetry-muted)]">{description}</span></div>;
}

function workingModeAnnouncement(mode: SocketWorkingMode): string {
  if (mode === "raw_relay") return "已切换为透明转发；协议处理设置已清除。";
  if (mode === "protocol_relay") return "已切换为按协议转发；请选择处理方案并配置需要执行的步骤。";
  return "已切换为本地应答；Server 地址和安全设置已清除。";
}

/**
 * Rust 返回稳定字段路径；这里按路径把消息归到用户实际操作的类别中，
 * 不向主流程暴露实现字段名，也不从自然语言猜测修复动作。
 */
function SocketFieldErrors({ errors }: { errors?: Record<string, string[]> }): ReactNode {
  const groups = [
    { label: "Socket 连接", tokens: ["topology", "downstream_security"] },
    // 规则路径也可能含 package/schema，必须先归入内容处理规则而不是包绑定。
    { label: "内容处理规则", tokens: ["socket_rules", "rule"] },
    { label: "精确协议包", tokens: ["package", "schema"] },
    { label: "处理选项", tokens: ["capability", "decode_enabled", "encode_enabled", "processing"] },
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
