"use client";

import { Alert, Button, Card, Switch } from "@heroui/react";
import type {
  CertificateReference,
  HttpListenerSettings,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
} from "@/generated/rust-types";
import {
  CertificateDetailPanel,
  CertificateReferenceSelect,
  ConnectionTestResult,
} from "./fixed-server-tls-fields";

const HTTPS_DESCRIPTION = [
  "本机代理作为 TLS 客户端连接固定 Server。",
  "Server 身份验证与代理客户端身份验证分别配置。",
].join("");
const SERVER_TRUST_DESCRIPTION = [
  "公开 HTTPS 可使用操作系统信任根；私有 CA 签发的 Server 证书需要导入对应 CA。",
  "该 CA 仅用于验证 Server 证书链。",
].join("");
const CLIENT_IDENTITY_DESCRIPTION = [
  "仅当上游 Server 要求 mTLS 时配置 client.p12 / client.pfx。",
  "该文件包含代理向 Server 出示的 clientAuth 客户端证书和私钥，不是 App 侧握手使用的 serverAuth 服务端身份。",
].join("");

type Props = {
  settings: HttpListenerSettings;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  busy: boolean;
  testing: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
  onOpenIdentityImport: () => void;
  onOpenTrustImport: () => void;
  onTest: () => Promise<void>;
};

export function UpstreamTlsCard(props: Props) {
  const fixedServer = props.settings.topology.mode === "remote_server"
    ? props.settings.topology.settings.fixed_server
    : null;
  if (!fixedServer) return null;
  const server = fixedServer;
  const identities = props.certificateReferences.filter(
    (reference) => reference.kind === "upstream_client_identity",
  );
  const trusts = props.certificateReferences.filter(
    (reference) => reference.kind === "upstream_server_trust",
  );
  const tls = server.upstream_tls;
  const usesTls = server.upstream_url.trim().toLowerCase().startsWith("https://");
  const trust = findReference(trusts, tls.server_trust);
  const identity = findReference(identities, tls.client_identity);

  function changeTls(changes: Partial<typeof tls>) {
    props.onChange({
      topology: { mode: "remote_server", settings: { fixed_server: {
          ...server,
          upstream_tls: { ...tls, ...changes },
        } } },
    });
  }

  return (
    <Card>
      <Card.Header>
        <Card.Title>Server 连接</Card.Title>
        <Card.Description>
          {usesTls
            ? HTTPS_DESCRIPTION
            : "本机代理连接固定 HTTP Server；当前地址不使用 TLS 证书或 mTLS 身份。"}
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-5">
        {usesTls && (
          <>
            <div
              className={[
                "flex items-center justify-between gap-4 rounded-xl px-4 py-3",
                "bg-[var(--telemetry-table-head)]",
              ].join(" ")}
            >
              <div>
                <p className="font-medium">校验上游 Server 主机名</p>
                <p className="text-sm text-[var(--telemetry-muted)]">
                  证书链验证始终执行；启用后同时验证 URL 主机名与证书标识是否一致。
                </p>
              </div>
              <Switch
                aria-label="校验上游服务器主机名"
                isSelected={tls.verify_hostname}
                onChange={(verifyHostname) =>
                  changeTls({ verify_hostname: verifyHostname })
                }
              >
                <Switch.Content>
                  <Switch.Control><Switch.Thumb /></Switch.Control>
                </Switch.Content>
              </Switch>
            </div>

            <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-4">
              <SectionHeading
                title="验证上游 Server 身份"
                description={SERVER_TRUST_DESCRIPTION}
              />
              <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-2 max-[620px]:grid-cols-1">
                <CertificateReferenceSelect
                  label="上游 Server 证书 CA"
                  value={tls.server_trust}
                  emptyLabel="使用操作系统信任根"
                  references={trusts}
                  onChange={(serverTrust) =>
                    changeTls({ server_trust: serverTrust })
                  }
                />
                <Button
                  variant="outline"
                  isDisabled={props.busy}
                  onPress={props.onOpenTrustImport}
                >
                  导入 Server CA
                </Button>
              </div>
              <CertificateDetailPanel
                reference={trust}
                detail={findDetail(props.certificateDetails, trust?.id)}
                emptyText="当前使用操作系统信任根，不绑定单独的上游 CA。"
              />
            </section>

            <section className="space-y-3 border-t border-[var(--telemetry-line)] pt-4">
              <SectionHeading
                title="上游 Server 验证代理身份（可选）"
                description={CLIENT_IDENTITY_DESCRIPTION}
              />
              <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-2 max-[620px]:grid-cols-1">
                <CertificateReferenceSelect
                  label="上游 mTLS 客户端身份"
                  value={tls.client_identity}
                  emptyLabel="不提供客户端证书（普通 TLS）"
                  references={identities}
                  onChange={(clientIdentity) =>
                    changeTls({ client_identity: clientIdentity })
                  }
                />
                <Button
                  variant="outline"
                  isDisabled={props.busy}
                  onPress={props.onOpenIdentityImport}
                >
                  导入客户端身份
                </Button>
              </div>
              <CertificateDetailPanel
                reference={identity}
                detail={findDetail(props.certificateDetails, identity?.id)}
                emptyText="当前不向上游 Server 提供客户端证书，使用普通 TLS。"
              />
            </section>
          </>
        )}

        <div className="flex flex-wrap items-center gap-3 border-t border-[var(--telemetry-line)] pt-4">
          <Button
            variant="outline"
            isDisabled={props.busy}
            onPress={() => void props.onTest()}
          >
            {props.testing ? "正在连接 Server…" : "测试 Server 连接"}
          </Button>
          <p className="text-xs text-[var(--telemetry-muted)]">
            使用当前监听配置真实连接 Server，只验证
            {usesTls ? " TCP + TLS" : " TCP"}，不发送 HTTP 业务请求。
          </p>
        </div>
        {props.testResult && (
          <ConnectionTestResult result={props.testResult} showTlsDetails={usesTls} />
        )}
        {props.testError && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>Server 连接测试失败</Alert.Title>
              <Alert.Description>{props.testError}</Alert.Description>
            </Alert.Content>
          </Alert>
        )}
      </Card.Content>
    </Card>
  );
}

function SectionHeading({ title, description }: { title: string; description: string }) {
  return (
    <div>
      <h3 className="font-semibold">{title}</h3>
      <p className="text-sm text-[var(--telemetry-muted)]">{description}</p>
    </div>
  );
}

function findReference(references: CertificateReference[], id?: string | null) {
  return references.find((reference) => reference.id === id);
}

function findDetail(details: ListenerCertificateDetailViewModel[], id?: string) {
  return details.find((detail) => detail.reference_id === id);
}
