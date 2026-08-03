"use client";

import { Card, Label, ListBox, Select, Switch } from "@heroui/react";
import type {
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ProxyListener,
} from "@/generated/rust-types";
import {
  CertificateDetailPanel,
  CertificateReferenceSelect,
} from "./fixed-server-tls-fields";

type Props = {
  listener: ProxyListener;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  onChange: (changes: Partial<ProxyListener>) => void;
};

export function DownstreamTlsCard({
  listener,
  certificateReferences,
  certificateDetails,
  onChange,
}: Props) {
  const identities = certificateReferences.filter(
    (reference) => reference.kind === "reverse_server_identity",
  );
  const trusts = certificateReferences.filter(
    (reference) => reference.kind === "downstream_client_trust",
  );
  const tls = listener.downstream_tls;
  const authentication = tls.client_authentication;
  const identity = findReference(identities, tls.server_identity);
  const clientTrust = authentication.mode === "disabled"
    ? undefined
    : findReference(trusts, authentication.trust);

  function changeAuthentication(mode: string) {
    const trust = trusts[0]?.id ?? "";
    onChange({
      downstream_tls: {
        ...tls,
        client_authentication: mode === "required"
          ? { mode: "required", trust }
          : mode === "optional"
            ? { mode: "optional", trust }
            : { mode: "disabled" },
      },
    });
  }

  return (
    <Card>
      <Card.Header>
        <Card.Title>客户端连接安全</Card.Title>
        <Card.Description>
          客户端 → 本机代理：决定此监听是否提供 TLS，以及代理是否验证客户端证书。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-5">
        <SettingSwitch
          selected={tls.enabled}
          title="为此监听启用 TLS"
          description="启用后，客户端必须使用 HTTPS/TLS 连接当前监听端口。"
          onChange={(enabled) => onChange({ downstream_tls: { ...tls, enabled } })}
        />
        {tls.enabled && (
          <>
            <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-4">
              <SectionHeading
                title="代理向客户端证明身份"
                description="选择本机代理在握手中出示的 Server 身份（服务端证书 + 私钥）。客户端必须信任其签发 CA。"
              />
              <CertificateReferenceSelect
                label="本监听服务端证书身份"
                value={tls.server_identity}
                emptyLabel="请选择服务端证书身份"
                references={identities}
                onChange={(serverIdentity) => onChange({
                  downstream_tls: { ...tls, server_identity: serverIdentity },
                })}
              />
              <CertificateDetailPanel
                reference={identity}
                detail={findDetail(certificateDetails, identity?.id)}
                emptyText="尚未选择服务端证书；客户端无法完成此监听的 TLS 握手。"
              />
            </section>

            <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-4">
              <SectionHeading
                title="验证客户端身份（可选）"
                description={authentication.mode === "disabled"
                  ? "普通 TLS 模式。代理不发送客户端证书请求，也不验证客户端证书；此模式不需要配置客户端证书 CA。"
                  : "代理使用签发客户端证书的 CA 验证证书链。client.p12 属于客户端身份，应安装在客户端。"}
              />
              <Select
                aria-label="下游客户端认证模式"
                selectedKey={authentication.mode}
                onSelectionChange={(key) => changeAuthentication(String(key))}
              >
                <Label>客户端证书要求</Label>
                <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
                <Select.Popover><ListBox>
                  <ListBox.Item id="disabled" textValue="不要求客户端证书">不要求客户端证书（普通 TLS）</ListBox.Item>
                  <ListBox.Item id="optional" textValue="客户端证书可选">客户端证书可选；出示时验证证书链</ListBox.Item>
                  <ListBox.Item id="required" textValue="必须验证客户端证书">必须出示并验证客户端证书（mTLS）</ListBox.Item>
                </ListBox></Select.Popover>
              </Select>
              {authentication.mode !== "disabled" && (
                <>
                  <CertificateReferenceSelect
                    label="用于验证客户端证书的 CA"
                    value={authentication.trust}
                    emptyLabel="请选择客户端证书 CA"
                    references={trusts}
                    onChange={(trust) => onChange({
                      downstream_tls: {
                        ...tls,
                        client_authentication: authentication.mode === "required"
                          ? { mode: "required", trust: trust ?? "" }
                          : { mode: "optional", trust: trust ?? "" },
                      },
                    })}
                  />
                  <CertificateDetailPanel
                    reference={clientTrust}
                    detail={findDetail(certificateDetails, clientTrust?.id)}
                    emptyText="尚未选择客户端证书 CA；无法校验客户端身份。"
                  />
                </>
              )}
            </section>
          </>
        )}
      </Card.Content>
    </Card>
  );
}

function SettingSwitch({ selected, title, description, onChange }: {
  selected: boolean;
  title: string;
  description: string;
  onChange: (selected: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3">
      <div><p className="font-medium">{title}</p><p className="text-sm text-[var(--telemetry-muted)]">{description}</p></div>
      <Switch aria-label={title} isSelected={selected} onChange={onChange}>
        <Switch.Content>
          <Switch.Control><Switch.Thumb /></Switch.Control>
        </Switch.Content>
      </Switch>
    </div>
  );
}

function SectionHeading({ title, description }: { title: string; description: string }) {
  return <div><h3 className="font-semibold">{title}</h3><p className="text-sm text-[var(--telemetry-muted)]">{description}</p></div>;
}

function findReference(references: CertificateReference[], id?: string | null) {
  return references.find((reference) => reference.id === id);
}

function findDetail(details: ListenerCertificateDetailViewModel[], id?: string) {
  return details.find((detail) => detail.reference_id === id);
}
