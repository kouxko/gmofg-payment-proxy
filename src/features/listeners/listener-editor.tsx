"use client";

import {
  Button,
  Card,
  Input,
  Label,
  NumberField,
  Switch,
} from "@heroui/react";
import type {
  CertificateItemViewModel,
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamTlsTestViewModel,
  ProxyListener,
} from "@/generated/rust-types";
import { BodyCodecSettings } from "./body-codec-settings";
import { FixedServerTlsSettings } from "./fixed-server-tls-settings";
import { RequestRoutingCard } from "./request-routing-card";

type Props = {
  listener: ProxyListener;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  installationLeaf?: CertificateItemViewModel;
  pending?: string;
  tlsTest?: ListenerUpstreamTlsTestViewModel;
  tlsTestError?: string;
  basicUsername: string;
  basicPassword: string;
  onBasicUsernameChange: (value: string) => void;
  onBasicPasswordChange: (value: string) => void;
  onChange: (changes: Partial<ProxyListener>) => void;
  onStoreBasicCredential: () => Promise<void>;
  onImportDownstreamServerIdentity: (label: string) => Promise<boolean>;
  onImportDownstreamClientTrust: (label: string) => Promise<boolean>;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTestUpstreamTls: () => Promise<void>;
};

export function ListenerEditor({
  listener,
  certificateReferences,
  certificateDetails,
  installationLeaf,
  pending,
  tlsTest,
  tlsTestError,
  basicUsername,
  basicPassword,
  onBasicUsernameChange,
  onBasicPasswordChange,
  onChange,
  onStoreBasicCredential,
  onImportDownstreamServerIdentity,
  onImportDownstreamClientTrust,
  onImportClientIdentity,
  onImportServerTrust,
  onTestUpstreamTls,
}: Props) {
  const basicCredential = listener.authentication.mode === "basic"
    ? listener.authentication.credential
    : undefined;
  return (
    <Card>
      <Card.Content className="grid grid-cols-2 gap-4 p-5 max-[700px]:grid-cols-1">
        <div className="grid gap-1">
          <Label>监听名称</Label>
          <Input
            aria-label="代理监听名称"
            value={listener.name}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="grid gap-1">
          <Label>绑定地址</Label>
          <Input
            aria-label="绑定地址"
            value={listener.bind_address}
            onChange={(event) => onChange({ bind_address: event.target.value })}
          />
        </div>
        <NumberField
          aria-label="监听端口"
          minValue={0}
          maxValue={65535}
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
        <RequestRoutingCard listener={listener} onChange={onChange} />

        <CommonListenerSettings
          listener={listener}
          pending={pending}
          basicUsername={basicUsername}
          basicPassword={basicPassword}
          onBasicUsernameChange={onBasicUsernameChange}
          onBasicPasswordChange={onBasicPasswordChange}
          onChange={onChange}
          onStoreBasicCredential={onStoreBasicCredential}
          basicCredentialKey={basicCredential?.key}
          basicCredentialProvider={basicCredential?.provider}
        />

        <BodyCodecSettings
          requestCodec={listener.request_body_codec}
          responseCodec={listener.response_body_codec}
          onRequestCodecChange={(requestBodyCodec) =>
            onChange({ request_body_codec: requestBodyCodec })
          }
          onResponseCodecChange={(responseBodyCodec) =>
            onChange({ response_body_codec: responseBodyCodec })
          }
        />
        <FixedServerTlsSettings
          listener={listener}
          certificateReferences={certificateReferences}
          certificateDetails={certificateDetails}
          installationLeaf={installationLeaf}
          busy={Boolean(pending)}
          testing={pending === "tls-test"}
          testResult={tlsTest}
          testError={tlsTestError}
          onChange={onChange}
          onImportDownstreamServerIdentity={onImportDownstreamServerIdentity}
          onImportDownstreamClientTrust={onImportDownstreamClientTrust}
          onImportClientIdentity={onImportClientIdentity}
          onImportServerTrust={onImportServerTrust}
          onTest={onTestUpstreamTls}
        />
      </Card.Content>
    </Card>
  );
}

function CommonListenerSettings({
  listener,
  pending,
  basicUsername,
  basicPassword,
  basicCredentialKey,
  basicCredentialProvider,
  onBasicUsernameChange,
  onBasicPasswordChange,
  onChange,
  onStoreBasicCredential,
}: Omit<Props, "certificateReferences" | "certificateDetails" | "installationLeaf" | "tlsTest" | "tlsTestError" | "onImportDownstreamServerIdentity" | "onImportDownstreamClientTrust" | "onImportClientIdentity" | "onImportServerTrust" | "onTestUpstreamTls"> & {
  basicCredentialKey?: string;
  basicCredentialProvider?: string;
}) {
  return (
    <>
      {(["connect", "read", "write"] as const).map((kind) => {
        const field = `${kind}_timeout_ms` as const;
        const labels = { connect: "连接超时", read: "读取超时", write: "写入超时" };
        return (
          <NumberField key={kind} aria-label={`${labels[kind]}毫秒`} minValue={0} value={listener[field]} onChange={(value) => onChange({ [field]: value })}>
            <Label>{labels[kind]}（ms）</Label>
            <NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
          </NumberField>
        );
      })}
      <div className="col-span-2 grid gap-1 max-[700px]:col-span-1">
        <Label>允许的客户端 CIDR</Label>
        <Input
          aria-label="允许的客户端 CIDR"
          value={listener.allowed_client_cidrs.join(", ")}
          onChange={(event) => onChange({ allowed_client_cidrs: splitValues(event.target.value) })}
          placeholder="127.0.0.1/32, 10.0.0.0/8"
        />
      </div>
      <div className="col-span-2 grid grid-cols-2 gap-4 rounded-2xl border border-[var(--telemetry-line)] p-4 max-[700px]:col-span-1 max-[700px]:grid-cols-1">
        <Switch
          isSelected={listener.authentication.mode === "basic"}
          onChange={(enabled) => onChange({ authentication: enabled ? { mode: "basic", credential: { provider: "system", key: "" } } : { mode: "none" } })}
        >
          <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>启用 HTTP Basic 认证</span></Switch.Content>
        </Switch>
        {!listener.fixed_server && (
          <Switch isSelected={listener.mitm.enabled} onChange={(enabled) => onChange({ mitm: { ...listener.mitm, enabled } })}>
            <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>启用 allowlist MITM</span></Switch.Content>
          </Switch>
        )}
        {listener.authentication.mode === "basic" && (
          <>
            <div className="grid gap-1"><Label>用户名</Label><Input aria-label="代理认证用户名" value={basicUsername} onChange={(event) => onBasicUsernameChange(event.target.value)} autoComplete="off" /></div>
            <div className="grid gap-1"><Label>密码</Label><Input aria-label="代理认证密码" type="password" value={basicPassword} onChange={(event) => onBasicPasswordChange(event.target.value)} autoComplete="new-password" /></div>
            <div className="col-span-2 flex items-center justify-between gap-3 max-[700px]:col-span-1">
              <p className="min-w-0 truncate text-xs text-[var(--telemetry-muted)]">{basicCredentialKey ? `已保存安全引用：${basicCredentialProvider}/${basicCredentialKey}` : "尚未保存凭据；明文不会写入 Workspace。"}</p>
              <Button variant="outline" isDisabled={!basicUsername || !basicPassword || Boolean(pending)} onPress={() => void onStoreBasicCredential()}>{pending === "secret" ? "保护中…" : "保护并引用"}</Button>
            </div>
          </>
        )}
        {!listener.fixed_server && listener.mitm.enabled && (
          <>
            <div className="col-span-2 grid gap-1 max-[700px]:col-span-1"><Label>MITM authority allowlist</Label><Input aria-label="MITM authority allowlist" value={listener.mitm.authority_allowlist.join(", ")} onChange={(event) => onChange({ mitm: { ...listener.mitm, authority_allowlist: splitValues(event.target.value) } })} placeholder="api.example.test, *.test.example" /></div>
            <NumberField aria-label="MITM 叶子证书缓存" minValue={1} maxValue={256} value={listener.mitm.maximum_cached_leaf_certificates} onChange={(maximum_cached_leaf_certificates) => onChange({ mitm: { ...listener.mitm, maximum_cached_leaf_certificates } })}><Label>叶子证书缓存</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
          </>
        )}
      </div>
    </>
  );
}

function splitValues(value: string) {
  return value.split(",").map((item) => item.trim()).filter(Boolean);
}
