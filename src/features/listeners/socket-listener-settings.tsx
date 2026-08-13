"use client";

import { useState, type ReactNode } from "react";
import { Alert, Button, Card, Input, Label, ListBox, NumberField, Select, Switch } from "@heroui/react";
import type {
  CertificateReference,
  DownstreamClientAuthentication,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
  SocketDownstreamTlsSettings,
  SocketRelaySecurity,
  SocketRelaySettings,
  SocketUpstreamTlsSettings,
} from "@/generated/rust-types";
import {
  CertificateDetailPanel,
  CertificateReferenceSelect,
  ConnectionTestResult,
} from "./fixed-server-tls-fields";
import { ImportIdentityModal, ImportPemModal, ImportTrustModal } from "./fixed-server-tls-import-modals";
import { changeSocketSecurity, socketDownstreamTls, socketUpstreamTls } from "./listener-data-plane";

type Props = {
  settings: SocketRelaySettings;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
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

const modes: Array<{ id: SocketRelaySecurity["mode"]; label: string }> = [
  { id: "transparent", label: "Transparent（TCP → TCP）" },
  { id: "tcp_to_tls", label: "TCP → TLS" },
  { id: "tls_to_tcp", label: "TLS → TCP" },
  { id: "tls_to_tls", label: "TLS → TLS" },
];

export function SocketListenerSettings(props: Props): ReactNode {
  const [modal, setModal] = useState<
    "downstream-identity" | "downstream-trust" | "upstream-identity" | "upstream-trust"
  >();
  const [label, setLabel] = useState("Socket TLS 证书");
  const [password, setPassword] = useState("");
  const downstream = socketDownstreamTls(props.settings.security);
  const upstream = socketUpstreamTls(props.settings.security);

  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      <Card>
        <Card.Header>
          <Card.Title>Socket 上游目标</Card.Title>
          <Card.Description>
            按原始字节双向转发，不解析 HTTP 方法、Header、正文或 query。
          </Card.Description>
        </Card.Header>
        <Card.Content className="grid grid-cols-2 gap-4 max-[700px]:grid-cols-1">
          <div className="grid gap-1">
            <Label>上游主机</Label>
            <Input
              aria-label="Socket 上游主机"
              value={props.settings.upstream.host}
              onChange={(event) => props.onChange({
                upstream: { ...props.settings.upstream, host: event.target.value },
              })}
            />
          </div>
          <SocketNumberField
            label="Socket 上游端口"
            value={props.settings.upstream.port}
            maximum={65535}
            onChange={(port) => props.onChange({ upstream: { ...props.settings.upstream, port } })}
          />
          <SocketNumberField
            label="最大并发连接"
            value={props.settings.maximum_connections}
            maximum={5000}
            onChange={(maximum_connections) => props.onChange({ maximum_connections })}
          />
          <Select
            aria-label="Socket 安全模式"
            selectedKey={props.settings.security.mode}
            onSelectionChange={(key) => props.onChange({
              security: changeSocketSecurity(
                String(key) as SocketRelaySecurity["mode"],
                props.settings.security,
              ),
            })}
          >
            <Label>安全桥接模式</Label>
            <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
            <Select.Popover><ListBox>
              {modes.map((mode) => (
                <ListBox.Item key={mode.id} id={mode.id} textValue={mode.label}>{mode.label}</ListBox.Item>
              ))}
            </ListBox></Select.Popover>
          </Select>
        </Card.Content>
      </Card>

      {props.settings.security.mode === "transparent" && (
        <Alert status="accent">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>Transparent：TLS 与明文均保持 opaque</Alert.Title>
            <Alert.Description>
              Relay 不终止 TLS，也不读取应用协议；双向字节保持 opaque，
              仅记录连接数和方向字节数。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      )}

      {downstream && (
        <DownstreamSocketTls
          tls={downstream}
          references={props.certificateReferences}
          details={props.certificateDetails}
          busy={props.busy}
          onChange={(next) => props.onChange({
            security: replaceDownstream(props.settings.security, next),
          })}
          onImportIdentity={() => openModal("downstream-identity", "Socket 服务端身份")}
          onImportTrust={() => openModal("downstream-trust", "Socket 客户端 CA")}
        />
      )}
      {upstream && (
        <UpstreamSocketTls
          tls={upstream}
          references={props.certificateReferences}
          details={props.certificateDetails}
          busy={props.busy}
          onChange={(next) => props.onChange({
            security: replaceUpstream(props.settings.security, next),
          })}
          onImportIdentity={() => openModal("upstream-identity", "Socket 上游客户端身份")}
          onImportTrust={() => openModal("upstream-trust", "Socket 上游 CA")}
        />
      )}

      <ConnectionProbe {...props} />
      {modal && renderImportModal()}
    </div>
  );

  function openModal(next: NonNullable<typeof modal>, nextLabel: string) {
    setLabel(nextLabel);
    setModal(next);
  }

  function closeModal(open: boolean) {
    if (!open) setModal(undefined);
  }

  async function runImport(action: () => Promise<boolean>) {
    if (!(await action())) return;
    setPassword("");
    setModal(undefined);
  }

  function renderImportModal() {
    if (modal === "upstream-identity") {
      return <ImportIdentityModal open busy={props.busy} label={label} password={password}
        onOpenChange={closeModal} onLabelChange={setLabel} onPasswordChange={setPassword}
        onImport={() => runImport(() => props.onImportClientIdentity(label, password))} />;
    }
    if (modal === "upstream-trust") {
      return <ImportTrustModal open busy={props.busy} label={label} onOpenChange={closeModal}
        onLabelChange={setLabel} onImport={() => runImport(() => props.onImportServerTrust(label))} />;
    }
    const identity = modal === "downstream-identity";
    return <ImportPemModal open busy={props.busy} label={label} onOpenChange={closeModal}
      onLabelChange={setLabel} title={identity ? "导入 Socket 服务端身份" : "导入 Socket 客户端 CA"}
      description={identity
        ? "选择同时包含服务端证书链与匹配私钥的 PEM identity。"
        : "选择用于验证下游客户端证书的 CA（CER / CRT / PEM / DER）。"}
      detail="文件会通过系统对话框读取，校验后保存为受保护引用。"
      buttonLabel={identity ? "选择服务端身份 PEM" : "选择客户端 CA"}
      onImport={() => runImport(() => identity
        ? props.onImportDownstreamServerIdentity(label)
        : props.onImportDownstreamClientTrust(label))} />;
  }
}

function DownstreamSocketTls({ tls, references, details, busy, onChange, onImportIdentity, onImportTrust }: {
  tls: SocketDownstreamTlsSettings;
  references: CertificateReference[];
  details: ListenerCertificateDetailViewModel[];
  busy: boolean;
  onChange: (tls: SocketDownstreamTlsSettings) => void;
  onImportIdentity: () => void;
  onImportTrust: () => void;
}): ReactNode {
  const identities = references.filter((item) => item.kind === "reverse_server_identity");
  const trusts = references.filter((item) => item.kind === "downstream_client_trust");
  const authentication = tls.client_authentication;
  const identity = identities.find((item) => item.id === tls.server_identity);
  const trustId = authentication.mode === "disabled" ? undefined : authentication.trust;
  const trust = trusts.find((item) => item.id === trustId);
  return <Card><Card.Header><Card.Title>客户端 → Relay TLS</Card.Title>
    <Card.Description>
      Relay 终止下游 TLS，因此必须配置服务端 PEM identity；mTLS 客户端 CA 可选。
    </Card.Description>
  </Card.Header><Card.Content className="space-y-4">
    <CertificateRow label="Socket 服务端身份" value={tls.server_identity || null}
      emptyLabel="请选择服务端 PEM identity" references={identities} button="导入服务端身份 PEM"
      busy={busy}
      onChange={(server_identity) => onChange({
        ...tls,
        server_identity: server_identity ?? "",
      })}
      onImport={onImportIdentity} />
    <CertificateDetailPanel
      reference={identity}
      detail={detail(details, identity?.id)}
      emptyText="尚未选择服务端身份。"
    />
    <ClientAuthentication value={authentication} trusts={trusts}
      onChange={(client_authentication) => onChange({ ...tls, client_authentication })} />
    {authentication.mode !== "disabled" && <>
      <CertificateRow
        label="Socket 下游客户端 CA"
        value={trustId ?? null}
        emptyLabel="请选择客户端 CA"
        references={trusts}
        button="导入下游客户端 CA"
        busy={busy}
        onChange={(nextTrust) => onChange({
          ...tls,
          client_authentication: authentication.mode === "required"
            ? { mode: "required", trust: nextTrust ?? "" }
            : { mode: "optional", trust: nextTrust ?? "" },
        })}
        onImport={onImportTrust}
      />
      <CertificateDetailPanel
        reference={trust}
        detail={detail(details, trust?.id)}
        emptyText="尚未选择客户端 CA。"
      />
    </>}
  </Card.Content></Card>;
}

function UpstreamSocketTls({ tls, references, details, busy, onChange, onImportIdentity, onImportTrust }: {
  tls: SocketUpstreamTlsSettings;
  references: CertificateReference[];
  details: ListenerCertificateDetailViewModel[];
  busy: boolean;
  onChange: (tls: SocketUpstreamTlsSettings) => void;
  onImportIdentity: () => void;
  onImportTrust: () => void;
}): ReactNode {
  const trusts = references.filter((item) => item.kind === "upstream_server_trust");
  const identities = references.filter((item) => item.kind === "upstream_client_identity");
  const trust = trusts.find((item) => item.id === tls.server_trust);
  const identity = identities.find((item) => item.id === tls.client_identity);
  return <Card><Card.Header><Card.Title>Relay → Server TLS</Card.Title>
    <Card.Description>Relay 建立上游 TLS，可绑定私有 CA 和可选 mTLS 客户端身份。</Card.Description>
  </Card.Header><Card.Content className="space-y-4">
    <div className="flex items-center justify-between gap-4 rounded-xl border border-[var(--telemetry-line)] p-3">
      <span>校验上游 Server 主机名</span>
      <Switch aria-label="校验 Socket 上游主机名" isSelected={tls.verify_hostname}
        onChange={(verify_hostname) => onChange({ ...tls, verify_hostname })}>
        <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content>
      </Switch>
    </div>
    <CertificateRow
      label="Socket 上游 Server CA"
      value={tls.server_trust}
      emptyLabel="使用系统信任根"
      references={trusts}
      button="导入上游 Server CA"
      busy={busy}
      onChange={(server_trust) => onChange({ ...tls, server_trust })}
      onImport={onImportTrust}
    />
    <CertificateDetailPanel
      reference={trust}
      detail={detail(details, trust?.id)}
      emptyText="当前使用系统信任根。"
    />
    <CertificateRow
      label="Socket 上游客户端身份"
      value={tls.client_identity}
      emptyLabel="不提供客户端身份"
      references={identities}
      button="导入上游客户端身份"
      busy={busy}
      onChange={(client_identity) => onChange({ ...tls, client_identity })}
      onImport={onImportIdentity}
    />
    <CertificateDetailPanel
      reference={identity}
      detail={detail(details, identity?.id)}
      emptyText="当前不提供 mTLS 客户端身份。"
    />
  </Card.Content></Card>;
}

function CertificateRow({ label, value, emptyLabel, references, button, busy, onChange, onImport }: {
  label: string; value: string | null; emptyLabel: string; references: CertificateReference[];
  button: string; busy: boolean; onChange: (value: string | null) => void; onImport: () => void;
}): ReactNode {
  return <div className={[
    "grid grid-cols-[minmax(0,1fr)_auto] items-end gap-2",
    "max-[620px]:grid-cols-1",
  ].join(" ")}>
    <CertificateReferenceSelect
      label={label}
      value={value}
      emptyLabel={emptyLabel}
      references={references}
      onChange={onChange}
    />
    <Button variant="outline" isDisabled={busy} onPress={onImport}>{button}</Button>
  </div>;
}

function ClientAuthentication({ value, trusts, onChange }: {
  value: DownstreamClientAuthentication;
  trusts: CertificateReference[];
  onChange: (value: DownstreamClientAuthentication) => void;
}): ReactNode {
  const trust = value.mode === "disabled" ? trusts[0]?.id ?? "" : value.trust;
  return <Select aria-label="Socket 下游客户端认证" selectedKey={value.mode}
    onSelectionChange={(key) => {
      onChange(clientAuthenticationFor(String(key), trust));
    }}>
    <Label>下游客户端证书要求</Label>
    <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
    <Select.Popover><ListBox>
      <ListBox.Item id="disabled">不要求客户端证书</ListBox.Item>
      <ListBox.Item id="optional">客户端证书可选</ListBox.Item>
      <ListBox.Item id="required">必须验证客户端证书</ListBox.Item>
    </ListBox></Select.Popover>
  </Select>;
}

function ConnectionProbe(props: Props): ReactNode {
  const mode = modes.find((item) => item.id === props.settings.security.mode)?.label;
  return <Card><Card.Content className="space-y-3 p-4">
    <div className="flex flex-wrap items-center gap-3">
      <Button variant="outline" isDisabled={props.busy} onPress={() => void props.onTest()}>
        {props.testing ? "正在探测 Socket 上游…" : "测试 Socket 上游连接"}
      </Button>
      <span className="text-xs text-[var(--telemetry-muted)]">
        真实建立 TCP；需要上游 TLS 的模式同时返回握手证据。
      </span>
    </div>
    {props.testResult && <><p className="text-xs text-[var(--telemetry-muted)]">桥接：{mode}</p>
      <ConnectionTestResult
        result={props.testResult}
        showTlsDetails={Boolean(socketUpstreamTls(props.settings.security))}
      /></>}
    {props.testError && <Alert status="danger"><Alert.Indicator /><Alert.Content>
      <Alert.Title>Socket 上游连接失败</Alert.Title><Alert.Description>{props.testError}</Alert.Description>
    </Alert.Content></Alert>}
  </Card.Content></Card>;
}

function SocketNumberField({ label, value, maximum, onChange }: {
  label: string; value: number; maximum: number; onChange: (value: number) => void;
}): ReactNode {
  return <NumberField aria-label={label} minValue={0} maxValue={maximum} value={value} onChange={onChange}>
    <Label>{label}</Label><NumberField.Group><NumberField.DecrementButton />
      <NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
  </NumberField>;
}

function replaceDownstream(
  security: SocketRelaySecurity,
  downstream_tls: SocketDownstreamTlsSettings,
): SocketRelaySecurity {
  return security.mode === "tls_to_tls"
    ? { ...security, downstream_tls }
    : { mode: "tls_to_tcp", downstream_tls };
}

function replaceUpstream(
  security: SocketRelaySecurity,
  upstream_tls: SocketUpstreamTlsSettings,
): SocketRelaySecurity {
  return security.mode === "tls_to_tls"
    ? { ...security, upstream_tls }
    : { mode: "tcp_to_tls", upstream_tls };
}

function clientAuthenticationFor(
  mode: string,
  trust: string,
): DownstreamClientAuthentication {
  switch (mode) {
    case "required":
    case "optional":
      return { mode, trust };
    default:
      return { mode: "disabled" };
  }
}

function detail(
  details: ListenerCertificateDetailViewModel[],
  id?: string,
): ListenerCertificateDetailViewModel | undefined {
  return details.find((item) => item.reference_id === id);
}
