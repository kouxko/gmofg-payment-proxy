"use client";

import { Button, Card, Input, Label, ListBox, NumberField, Select, Switch } from "@heroui/react";
import type { ReactNode } from "react";
import type {
  CertificateItemViewModel,
  CertificateReference,
  HttpListenerSettings,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
  ProxyListener,
} from "@/generated/rust-types";
import { BodyCodecSettings } from "./body-codec-settings";
import { FixedServerTlsSettings } from "./fixed-server-tls-settings";
import { HttpProtocolProcessingCard } from "./http-protocol-processing-card";
import {
  changeDataPlaneKind,
  changeHttpSettings,
  changeSocketSettings,
} from "./listener-data-plane";
import { RequestRoutingCard } from "./request-routing-card";
import { SocketListenerSettings } from "./socket-listener-settings";
import type { ProtocolCatalogState } from "./socket-processing-card";

const timeoutFields = [
  { key: "connect", field: "connect_timeout_ms", label: "连接超时" },
  { key: "read", field: "read_timeout_ms", label: "读取超时" },
  { key: "write", field: "write_timeout_ms", label: "写入超时" },
] as const;

type Props = {
  listener: ProxyListener;
  protocolCatalog?: ProtocolCatalogState;
  locked?: boolean;
  fieldErrors?: Record<string, string[]>;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  installationRoot?: CertificateItemViewModel;
  pending?: string;
  tlsTest?: ListenerUpstreamConnectionTestViewModel;
  tlsTestError?: string;
  basicUsername: string;
  basicPassword: string;
  onBasicUsernameChange: (value: string) => void;
  onBasicPasswordChange: (value: string) => void;
  onChange: (changes: Partial<ProxyListener>) => void;
  onStoreBasicCredential: () => Promise<void>;
  onImportDownstreamServerIdentity: (label: string, password: string) => Promise<boolean>;
  onImportDownstreamClientTrust: (label: string) => Promise<boolean>;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTestUpstreamTls: () => Promise<void>;
};

export function ListenerEditor(props: Props): ReactNode {
  const { listener } = props;
  const locked = props.locked ?? false;
  const protocolCatalog = props.protocolCatalog ?? {
    loading: false,
    refresh: async () => undefined,
  };
  const http = listener.data_plane.kind === "http" ? listener.data_plane.settings : undefined;
  const socket = listener.data_plane.kind === "socket" ? listener.data_plane.settings : undefined;
  const changeHttp = (changes: Partial<HttpListenerSettings>) => {
    if (http) props.onChange(changeHttpSettings(http, changes));
  };

  return (
    <Card>
      <Card.Content className="grid grid-cols-2 gap-4 p-5 max-[700px]:grid-cols-1">
        <CommonFields listener={listener} locked={locked} onChange={props.onChange} />
        <Select
          aria-label="监听数据平面"
          isDisabled={locked}
          selectedKey={listener.data_plane.kind}
          onSelectionChange={(key) => {
            if (key === "http" || key === "socket") {
              props.onChange(changeDataPlaneKind(listener, key));
            }
          }}
        >
          <Label>数据平面</Label>
          <Select.Trigger className="h-10 min-h-10"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>
            <ListBox.Item id="http" textValue="HTTP 代理">HTTP 代理</ListBox.Item>
            <ListBox.Item id="socket" textValue="Socket 转发">Socket 转发</ListBox.Item>
          </ListBox></Select.Popover>
        </Select>
        {http && <fieldset disabled={locked} className="contents">
          <HttpSettings
            {...props}
            settings={http}
            protocolCatalog={protocolCatalog}
            onSettingsChange={changeHttp}
          />
        </fieldset>}
        {socket && (
          <SocketListenerSettings
            settings={socket}
            certificateReferences={props.certificateReferences}
            certificateDetails={props.certificateDetails}
            protocolCatalog={protocolCatalog}
            locked={locked}
            fieldErrors={props.fieldErrors}
            busy={Boolean(props.pending)}
            testing={props.pending === "tls-test"}
            testResult={props.tlsTest}
            testError={props.tlsTestError}
            onChange={(changes) => props.onChange(changeSocketSettings(socket, changes))}
            onImportDownstreamServerIdentity={props.onImportDownstreamServerIdentity}
            onImportDownstreamClientTrust={props.onImportDownstreamClientTrust}
            onImportClientIdentity={props.onImportClientIdentity}
            onImportServerTrust={props.onImportServerTrust}
            onTest={props.onTestUpstreamTls}
          />
        )}
      </Card.Content>
    </Card>
  );
}

function CommonFields({ listener, locked = false, onChange }: Pick<Props, "listener" | "locked" | "onChange">): ReactNode {
  return <>
    <div className="grid gap-1">
      <Label>监听名称</Label>
      <Input
        aria-label="代理监听名称"
        disabled={locked}
        value={listener.name}
        onChange={(event) => onChange({ name: event.target.value })}
      />
    </div>
    <div className="grid gap-1">
      <Label>绑定地址</Label>
      <Input
        aria-label="绑定地址"
        disabled={locked}
        value={listener.bind_address}
        onChange={(event) => onChange({ bind_address: event.target.value })}
      />
    </div>
    <NumberField
      aria-label="监听端口"
      minValue={0}
      maxValue={65535}
      isDisabled={locked}
      value={listener.port}
      onChange={(port) => onChange({ port })}
    >
      <Label>监听端口</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
    {timeoutFields.map(({ key, field, label }) => (
      <TimeoutField
        key={key}
        label={label}
        value={listener[field]}
        disabled={locked}
        onChange={(value) => onChange({ [field]: value })}
      />
    ))}
  </>;
}

function HttpSettings(props: Props & {
  settings: HttpListenerSettings;
  protocolCatalog: ProtocolCatalogState;
  onSettingsChange: (changes: Partial<HttpListenerSettings>) => void;
}): ReactNode {
  const { settings, onSettingsChange } = props;
  const credential = settings.authentication.mode === "basic"
    ? settings.authentication.credential
    : undefined;
  return <>
    <RequestRoutingCard settings={settings} onChange={onSettingsChange} />
    <HttpAuthentication
      settings={settings}
      pending={props.pending}
      username={props.basicUsername}
      password={props.basicPassword}
      credentialKey={credential?.key}
      credentialProvider={credential?.provider}
      onUsernameChange={props.onBasicUsernameChange}
      onPasswordChange={props.onBasicPasswordChange}
      onChange={onSettingsChange}
      onStore={props.onStoreBasicCredential}
    />
    <HttpProtocolProcessingCard
      settings={settings}
      catalog={props.protocolCatalog}
      locked={props.locked ?? false}
      onChange={onSettingsChange}
    />
    <BodyCodecSettings
      requestCodec={settings.request_body_codec}
      responseCodec={settings.response_body_codec}
      onRequestCodecChange={(request_body_codec) => onSettingsChange({ request_body_codec })}
      onResponseCodecChange={(response_body_codec) => onSettingsChange({ response_body_codec })}
    />
    <FixedServerTlsSettings
      settings={settings}
      certificateReferences={props.certificateReferences}
      certificateDetails={props.certificateDetails}
      installationRoot={props.installationRoot}
      busy={Boolean(props.pending)}
      testing={props.pending === "tls-test"}
      testResult={props.tlsTest}
      testError={props.tlsTestError}
      onChange={onSettingsChange}
      onImportDownstreamServerIdentity={props.onImportDownstreamServerIdentity}
      onImportDownstreamClientTrust={props.onImportDownstreamClientTrust}
      onImportClientIdentity={props.onImportClientIdentity}
      onImportServerTrust={props.onImportServerTrust}
      onTest={props.onTestUpstreamTls}
    />
  </>;
}

function HttpAuthentication({
  settings,
  pending,
  username,
  password,
  credentialKey,
  credentialProvider,
  onUsernameChange,
  onPasswordChange,
  onChange,
  onStore,
}: {
  settings: HttpListenerSettings;
  pending?: string;
  username: string;
  password: string;
  credentialKey?: string;
  credentialProvider?: string;
  onUsernameChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
  onStore: () => Promise<void>;
}): ReactNode {
  return <div className={[
    "col-span-2 grid grid-cols-2 gap-4 rounded-2xl border",
    "border-[var(--telemetry-line)] p-4",
    "max-[700px]:col-span-1 max-[700px]:grid-cols-1",
  ].join(" ")}>
    <div
      role="group"
      aria-label="HTTP Basic 认证开关"
      className="col-span-2 max-[700px]:col-span-1"
    >
      <Switch
        isSelected={settings.authentication.mode === "basic"}
        onChange={(enabled) => onChange({
          authentication: enabled
            ? { mode: "basic", credential: { provider: "system", key: "" } }
            : { mode: "none" },
        })}
      >
        <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control>
          <span>启用 HTTP Basic 认证</span></Switch.Content>
      </Switch>
    </div>
    {settings.topology.mode === "remote_server" && !settings.topology.settings.fixed_server && <MitmSwitch settings={settings} onChange={onChange} />}
    {settings.authentication.mode === "basic" && <>
      <div
        role="group"
        aria-label="HTTP Basic 认证凭据"
        className={[
          "col-span-2 grid grid-cols-2 gap-4",
          "max-[700px]:col-span-1 max-[700px]:grid-cols-1",
        ].join(" ")}
      >
        <TextInput label="用户名" ariaLabel="代理认证用户名" value={username} onChange={onUsernameChange} />
        <TextInput label="密码" ariaLabel="代理认证密码" value={password} onChange={onPasswordChange} password />
      </div>
      <div className="col-span-2 flex items-center justify-between gap-3 max-[700px]:col-span-1">
        <p className="min-w-0 truncate text-xs text-[var(--telemetry-muted)]">
          {credentialKey
            ? `已保存安全引用：${credentialProvider}/${credentialKey}`
            : "尚未保存凭据；明文不会写入 Workspace。"}
        </p>
        <Button
          variant="outline"
          isDisabled={!username || !password || Boolean(pending)}
          onPress={() => void onStore()}
        >
          {pending === "secret" ? "保护中…" : "保护并引用"}
        </Button>
      </div>
    </>}
    {settings.topology.mode === "remote_server" && !settings.topology.settings.fixed_server && settings.mitm.enabled && <MitmFields settings={settings} onChange={onChange} />}
  </div>;
}

function MitmSwitch({ settings, onChange }: {
  settings: HttpListenerSettings;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
}): ReactNode {
  return <Switch isSelected={settings.mitm.enabled} onChange={(enabled) => onChange({
    mitm: { ...settings.mitm, enabled },
  })}>
    <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control>
      <span>启用 allowlist MITM</span></Switch.Content>
  </Switch>;
}

function MitmFields({ settings, onChange }: {
  settings: HttpListenerSettings;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
}): ReactNode {
  return <>
    <div className="col-span-2 grid gap-1 max-[700px]:col-span-1">
      <Label>MITM authority allowlist</Label>
      <Input aria-label="MITM authority allowlist" value={settings.mitm.authority_allowlist.join(", ")}
        onChange={(event) => onChange({ mitm: {
          ...settings.mitm,
          authority_allowlist: splitValues(event.target.value),
        } })} placeholder="api.example.test, *.test.example" />
    </div>
    <NumberField aria-label="MITM 叶子证书缓存" minValue={1} maxValue={256}
      value={settings.mitm.maximum_cached_leaf_certificates}
      onChange={(maximum_cached_leaf_certificates) => onChange({ mitm: {
        ...settings.mitm,
        maximum_cached_leaf_certificates,
      } })}>
      <Label>叶子证书缓存</Label><NumberField.Group><NumberField.DecrementButton />
        <NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
    </NumberField>
  </>;
}

function TextInput({ label, ariaLabel, value, onChange, password = false }: {
  label: string; ariaLabel: string; value: string; onChange: (value: string) => void; password?: boolean;
}): ReactNode {
  return <div className="grid gap-1"><Label>{label}</Label><Input aria-label={ariaLabel} className="h-10 py-0"
    type={password ? "password" : "text"} value={value} autoComplete={password ? "new-password" : "off"}
    onChange={(event) => onChange(event.target.value)} /></div>;
}

function TimeoutField({ label, value, disabled, onChange }: {
  label: string; value: number; disabled: boolean; onChange: (value: number) => void;
}): ReactNode {
  return <NumberField aria-label={`${label}毫秒`} minValue={0} value={value} isDisabled={disabled} onChange={onChange}>
    <Label>{label}（ms）</Label><NumberField.Group><NumberField.DecrementButton />
      <NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
  </NumberField>;
}

function splitValues(value: string): string[] {
  return value.split(",").map((item) => item.trim()).filter(Boolean);
}
