"use client";

import { useState } from "react";
import { Alert, Button, Card, Input, Label, ListBox, NumberField, Select, Switch } from "@heroui/react";
import type {
  CertificateReference,
  DownstreamClientAuthentication,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
  SocketDownstreamTlsSettings,
  SocketRelaySettings,
  SocketUpstreamTlsSettings,
} from "@/generated/rust-types";
import { CertificateDetailPanel, CertificateReferenceSelect, ConnectionTestResult } from "./fixed-server-tls-fields";
import { ImportIdentityModal, ImportPemModal, ImportTrustModal } from "./fixed-server-tls-import-modals";
import { socketUpstreamTls } from "./listener-data-plane";
import {
  appSecurity,
  setAppTls,
  setAppTransport,
  setServerTls,
  setServerTransport,
} from "./socket-listener-model";

interface CommonProps {
  settings: SocketRelaySettings;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  locked: boolean;
  busy: boolean;
  onChange: (settings: SocketRelaySettings) => void;
}

export function SocketAppSecurityCard(props: CommonProps & {
  onImportIdentity: (label: string, password: string) => Promise<boolean>;
  onImportTrust: (label: string) => Promise<boolean>;
}) {
  const [modal, setModal] = useState<"identity" | "trust">();
  const [label, setLabel] = useState("Socket App TLS 证书");
  const [password, setPassword] = useState("");
  const security = appSecurity(props.settings);
  const tls = security.mode === "tls" ? security.downstream_tls : undefined;
  return <Card><Card.Header><Card.Title>2. App 接入安全</Card.Title>
    <Card.Description>入口面向 App 的接入方式；TLS 可选择客户端证书认证。</Card.Description>
  </Card.Header><Card.Content className="space-y-4">
    <TransportSelect label="App 接入传输" value={security.mode} disabled={props.locked}
      onChange={(mode) => props.onChange(setAppTransport(props.settings, mode))} />
    {tls && <DownstreamTlsFields tls={tls} references={props.certificateReferences}
      details={props.certificateDetails} disabled={props.locked || props.busy}
      onChange={(next) => props.onChange(setAppTls(props.settings, next))}
      onImportIdentity={() => { setLabel("Socket App 服务端身份"); setModal("identity"); }}
      onImportTrust={() => { setLabel("Socket App 客户端 CA"); setModal("trust"); }} />}
    {modal === "identity" && <ImportIdentityModal open busy={props.busy} label={label} password={password}
      onOpenChange={(open) => { if (!open) { setPassword(""); setModal(undefined); } }}
      onLabelChange={setLabel} onPasswordChange={setPassword}
      title="导入 App 侧服务端身份"
      description="选择 .p12 / .pfx，或同时包含服务端证书链与匹配私钥的 .pem。入口接受 App TLS 连接时出示它；它必须具备 serverAuth，而不是上游 mTLS 使用的 clientAuth 身份。"
      detail="文件经系统对话框读取并保存为受保护引用。输入的密码仅用于本次解密，不写入入口、工作区或诊断信息。"
      buttonLabel="选择服务端身份（.p12 / .pfx / .pem）"
      buttonAriaLabel="选择服务端身份（.p12 / .pfx / .pem）"
      onImport={async () => { const ok = await props.onImportIdentity(label, password); setPassword(""); if (ok) setModal(undefined); }} />}
    {modal === "trust" && <ImportPemModal open busy={props.busy} label={label}
      onOpenChange={(open) => { if (!open) setModal(undefined); }} onLabelChange={setLabel}
      title="导入 App 客户端 CA" description="选择用于验证 App 客户端证书的 CA。"
      detail="文件经系统对话框读取并保存为受保护引用。" buttonLabel="选择客户端 CA"
      onImport={async () => { if (await props.onImportTrust(label)) setModal(undefined); }} />}
  </Card.Content></Card>;
}

export function SocketServerCard(props: CommonProps & {
  testing: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
  onImportIdentity: (label: string, password: string) => Promise<boolean>;
  onImportTrust: (label: string) => Promise<boolean>;
  onTest: () => Promise<void>;
}) {
  const [modal, setModal] = useState<"identity" | "trust">();
  const [label, setLabel] = useState("Socket Server TLS 证书");
  const [password, setPassword] = useState("");
  // Hooks 必须在拓扑条件分支之前执行，Relay/LocalResponder 动态切换才不会改变调用顺序。
  if (props.settings.topology.mode !== "relay") return null;
  const relay = props.settings.topology.settings;
  const tls = socketUpstreamTls(relay.security);
  function changeRelay(changes: Partial<typeof relay>) {
    props.onChange({ ...props.settings, topology: { mode: "relay", settings: { ...relay, ...changes } } });
  }
  return <Card><Card.Header><Card.Title>3. Server 上游</Card.Title>
    <Card.Description>仅转发到远端时建立 Server 连接；本地应答不会保留此卡字段。</Card.Description>
  </Card.Header><Card.Content className="space-y-4">
    <div className="grid gap-4 md:grid-cols-2">
      <div className="grid gap-1"><Label>Server 主机</Label><Input aria-label="Socket Server 主机" disabled={props.locked}
        value={relay.upstream.host} onChange={(event) => changeRelay({ upstream: { ...relay.upstream, host: event.target.value } })} /></div>
      <NumberField aria-label="Socket Server 端口" minValue={0} maxValue={65535} value={relay.upstream.port}
        isDisabled={props.locked} onChange={(port) => changeRelay({ upstream: { ...relay.upstream, port } })}>
        <Label>Server 端口</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
      </NumberField>
      <TransportSelect label="Server 传输" value={tls ? "tls" : "tcp"} disabled={props.locked}
        onChange={(mode) => props.onChange(setServerTransport(props.settings, mode))} />
    </div>
    {tls && <UpstreamTlsFields tls={tls} references={props.certificateReferences} details={props.certificateDetails}
      disabled={props.locked || props.busy} onChange={(next) => props.onChange(setServerTls(props.settings, next))}
      onImportIdentity={() => { setLabel("Socket Server 客户端身份"); setModal("identity"); }}
      onImportTrust={() => { setLabel("Socket Server CA"); setModal("trust"); }} />}
    <div className="flex flex-wrap items-center gap-3"><Button variant="outline" isDisabled={props.locked || props.busy} onPress={() => void props.onTest()}>
      {props.testing ? "正在探测 Server…" : "测试 Server 连接"}</Button>
      <span className="text-xs text-[var(--telemetry-muted)]">执行 DNS 与 TCP 连接；Server TLS 模式同时验证握手。</span></div>
    {props.testResult && <ConnectionTestResult result={props.testResult} showTlsDetails={Boolean(tls)} />}
    {props.testError && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>Server 连接失败</Alert.Title><Alert.Description>{props.testError}</Alert.Description></Alert.Content></Alert>}
    {modal === "identity" && <ImportIdentityModal open busy={props.busy} label={label} password={password}
      onOpenChange={(open) => { if (!open) { setPassword(""); setModal(undefined); } }} onLabelChange={setLabel} onPasswordChange={setPassword}
      onImport={async () => { const ok = await props.onImportIdentity(label, password); setPassword(""); if (ok) setModal(undefined); }} />}
    {modal === "trust" && <ImportTrustModal open busy={props.busy} label={label}
      onOpenChange={(open) => { if (!open) setModal(undefined); }} onLabelChange={setLabel}
      onImport={async () => { if (await props.onImportTrust(label)) setModal(undefined); }} />}
  </Card.Content></Card>;
}

function DownstreamTlsFields({ tls, references, details, disabled, onChange, onImportIdentity, onImportTrust }: {
  tls: SocketDownstreamTlsSettings; references: CertificateReference[]; details: ListenerCertificateDetailViewModel[];
  disabled: boolean; onChange: (value: SocketDownstreamTlsSettings) => void; onImportIdentity: () => void; onImportTrust: () => void;
}) {
  const identities = references.filter((item) => item.kind === "reverse_server_identity");
  const trusts = references.filter((item) => item.kind === "downstream_client_trust");
  const authentication = tls.client_authentication;
  const trustId = authentication.mode === "disabled" ? undefined : authentication.trust;
  return <div className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
    <p className="text-xs text-[var(--telemetry-muted)]">App 侧使用 serverAuth 服务端身份；不要选择代理连接上游 Server 时使用的 clientAuth 客户端身份。</p>
    <CertificateRow label="App 侧服务端身份" value={tls.server_identity || null} emptyLabel="请选择服务端 PEM identity"
      references={identities} button="导入服务端身份" disabled={disabled}
      onChange={(server_identity) => onChange({ ...tls, server_identity: server_identity ?? "" })} onImport={onImportIdentity} />
    <CertificateDetailPanel reference={identities.find((item) => item.id === tls.server_identity)} detail={detail(details, tls.server_identity)} emptyText="尚未选择 App 侧服务端身份。" />
    <ClientAuthentication value={authentication} trusts={trusts} disabled={disabled} onChange={(client_authentication) => onChange({ ...tls, client_authentication })} />
    {authentication.mode !== "disabled" && <><CertificateRow label="App 客户端 CA" value={trustId ?? null} emptyLabel="请选择客户端 CA"
      references={trusts} button="导入客户端 CA" disabled={disabled} onImport={onImportTrust}
      onChange={(trust) => onChange({ ...tls, client_authentication: authentication.mode === "required" ? { mode: "required", trust: trust ?? "" } : { mode: "optional", trust: trust ?? "" } })} />
      <CertificateDetailPanel reference={trusts.find((item) => item.id === trustId)} detail={detail(details, trustId)} emptyText="尚未选择 App 客户端 CA。" /></>}
  </div>;
}

function UpstreamTlsFields({ tls, references, details, disabled, onChange, onImportIdentity, onImportTrust }: {
  tls: SocketUpstreamTlsSettings; references: CertificateReference[]; details: ListenerCertificateDetailViewModel[];
  disabled: boolean; onChange: (value: SocketUpstreamTlsSettings) => void; onImportIdentity: () => void; onImportTrust: () => void;
}) {
  const trusts = references.filter((item) => item.kind === "upstream_server_trust");
  const identities = references.filter((item) => item.kind === "upstream_client_identity");
  return <div className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
    <p className="text-xs text-[var(--telemetry-muted)]">这里的身份用于代理连接上游 Server：mTLS 身份必须具备 clientAuth，不是 App 接入时使用的 serverAuth 服务端身份。</p>
    <div className="flex items-center justify-between gap-4"><span>校验 Server 主机名</span><Switch aria-label="校验 Socket Server 主机名" isSelected={tls.verify_hostname} isDisabled={disabled} onChange={(verify_hostname) => onChange({ ...tls, verify_hostname })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content></Switch></div>
    <CertificateRow label="Server CA" value={tls.server_trust} emptyLabel="使用系统信任根" references={trusts} button="导入 Server CA" disabled={disabled} onChange={(server_trust) => onChange({ ...tls, server_trust })} onImport={onImportTrust} />
    <CertificateDetailPanel reference={trusts.find((item) => item.id === tls.server_trust)} detail={detail(details, tls.server_trust ?? undefined)} emptyText="当前使用系统信任根。" />
    <CertificateRow label="Server mTLS 客户端身份" value={tls.client_identity} emptyLabel="不提供客户端身份" references={identities} button="导入客户端身份" disabled={disabled} onChange={(client_identity) => onChange({ ...tls, client_identity })} onImport={onImportIdentity} />
    <CertificateDetailPanel reference={identities.find((item) => item.id === tls.client_identity)} detail={detail(details, tls.client_identity ?? undefined)} emptyText="当前不提供 mTLS 客户端身份。" />
  </div>;
}

function TransportSelect({ label, value, disabled, onChange }: { label: string; value: "tcp" | "tls"; disabled: boolean; onChange: (value: "tcp" | "tls") => void }) {
  return <Select aria-label={label} selectedKey={value} isDisabled={disabled} onSelectionChange={(key) => {
    // 只接受组件声明过的精确枚举，避免 null/未知 key 被强转为非法配置。
    if (key === "tcp" || key === "tls") onChange(key);
  }}><Label>{label}</Label><Select.Trigger className="h-10 min-h-10"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="tcp" textValue="TCP">TCP</ListBox.Item><ListBox.Item id="tls" textValue="TLS">TLS</ListBox.Item></ListBox></Select.Popover></Select>;
}

function CertificateRow({ label, value, emptyLabel, references, button, disabled, onChange, onImport }: {
  label: string; value: string | null; emptyLabel: string; references: CertificateReference[]; button: string;
  disabled: boolean; onChange: (value: string | null) => void; onImport: () => void;
}) {
  return <div className="grid items-end gap-2 sm:grid-cols-[minmax(0,1fr)_auto]"><CertificateReferenceSelect label={label} value={value} emptyLabel={emptyLabel} references={references} isDisabled={disabled} onChange={onChange} /><Button variant="outline" isDisabled={disabled} onPress={onImport}>{button}</Button></div>;
}

function ClientAuthentication({ value, trusts, disabled, onChange }: {
  value: DownstreamClientAuthentication; trusts: CertificateReference[]; disabled: boolean; onChange: (value: DownstreamClientAuthentication) => void;
}) {
  const trust = value.mode === "disabled" ? trusts[0]?.id ?? "" : value.trust;
  return <Select aria-label="App 客户端证书要求" selectedKey={value.mode} isDisabled={disabled} onSelectionChange={(key) => {
    if (key === "required" || key === "optional") onChange({ mode: key, trust });
    else if (key === "disabled") onChange({ mode: "disabled" });
  }}><Label>App 客户端证书要求</Label><Select.Trigger className="h-10 min-h-10"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox><ListBox.Item id="disabled" textValue="不要求客户端证书">不要求客户端证书</ListBox.Item><ListBox.Item id="optional" textValue="客户端证书可选">客户端证书可选</ListBox.Item><ListBox.Item id="required" textValue="必须验证客户端证书">必须验证客户端证书</ListBox.Item></ListBox></Select.Popover></Select>;
}

function detail(details: ListenerCertificateDetailViewModel[], id?: string): ListenerCertificateDetailViewModel | undefined {
  return details.find((item) => item.reference_id === id);
}
