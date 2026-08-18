"use client";

/**
 * 当前监听的双向 TLS 配置编排。
 *
 * 下游与上游是两次独立握手，因此分别展示“谁向谁证明身份”。证书文件选择、解析、
 * 密钥保护和真实握手全部由 Rust 执行；本组件只维护弹窗与用户输入状态。
 */

import { useState } from "react";
import type {
  CertificateItemViewModel,
  CertificateReference,
  HttpListenerSettings,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
} from "@/generated/rust-types";
import { DownstreamTlsCard } from "./downstream-tls-card";
import {
  ImportIdentityModal,
  ImportPemModal,
  ImportTrustModal,
} from "./fixed-server-tls-import-modals";
import { UpstreamTlsCard } from "./upstream-tls-card";

type Props = {
  settings: HttpListenerSettings;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  installationRoot?: CertificateItemViewModel;
  busy: boolean;
  testing: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
  onChange: (changes: Partial<HttpListenerSettings>) => void;
  onImportDownstreamServerIdentity: (label: string, password: string) => Promise<boolean>;
  onImportDownstreamClientTrust: (label: string) => Promise<boolean>;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTest: () => Promise<void>;
};

export function FixedServerTlsSettings(props: Props) {
  const [downstreamIdentityOpen, setDownstreamIdentityOpen] = useState(false);
  const [downstreamIdentityLabel, setDownstreamIdentityLabel] = useState("本监听独立服务端身份");
  const [downstreamIdentityPassword, setDownstreamIdentityPassword] = useState("");
  const [downstreamTrustOpen, setDownstreamTrustOpen] = useState(false);
  const [downstreamTrustLabel, setDownstreamTrustLabel] = useState("客户端证书 CA");
  const [identityOpen, setIdentityOpen] = useState(false);
  const [identityLabel, setIdentityLabel] = useState("上游 mTLS 客户端身份");
  const [identityPassword, setIdentityPassword] = useState("");
  const [trustOpen, setTrustOpen] = useState(false);
  const [trustLabel, setTrustLabel] = useState("上游服务器 CA");

  async function importIdentity() {
    const imported = await props.onImportClientIdentity(identityLabel, identityPassword);
    setIdentityPassword("");
    if (!imported) return;
    setIdentityOpen(false);
  }

  function changeIdentityOpen(open: boolean) {
    if (!open) setIdentityPassword("");
    setIdentityOpen(open);
  }

  async function importTrust() {
    if (!(await props.onImportServerTrust(trustLabel))) return;
    setTrustOpen(false);
  }

  async function importDownstreamIdentity() {
    const imported = await props.onImportDownstreamServerIdentity(
      downstreamIdentityLabel,
      downstreamIdentityPassword,
    );
    setDownstreamIdentityPassword("");
    if (!imported) return;
    setDownstreamIdentityOpen(false);
  }

  function changeDownstreamIdentityOpen(open: boolean) {
    if (!open) setDownstreamIdentityPassword("");
    setDownstreamIdentityOpen(open);
  }

  async function importDownstreamTrust() {
    if (!(await props.onImportDownstreamClientTrust(downstreamTrustLabel))) return;
    setDownstreamTrustOpen(false);
  }

  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      <DownstreamTlsCard
        settings={props.settings}
        certificateReferences={props.certificateReferences}
        certificateDetails={props.certificateDetails}
        installationRoot={props.installationRoot}
        busy={props.busy}
        onChange={props.onChange}
        onOpenIdentityImport={() => setDownstreamIdentityOpen(true)}
        onOpenTrustImport={() => setDownstreamTrustOpen(true)}
      />
      <UpstreamTlsCard
        settings={props.settings}
        certificateReferences={props.certificateReferences}
        certificateDetails={props.certificateDetails}
        busy={props.busy}
        testing={props.testing}
        testResult={props.testResult}
        testError={props.testError}
        onChange={props.onChange}
        onOpenIdentityImport={() => setIdentityOpen(true)}
        onOpenTrustImport={() => setTrustOpen(true)}
        onTest={props.onTest}
      />
      {identityOpen && (
        <ImportIdentityModal
          open
          busy={props.busy}
          label={identityLabel}
          password={identityPassword}
          onOpenChange={changeIdentityOpen}
          onLabelChange={setIdentityLabel}
          onPasswordChange={setIdentityPassword}
          onImport={importIdentity}
        />
      )}
      {trustOpen && (
        <ImportTrustModal
          open
          busy={props.busy}
          label={trustLabel}
          onOpenChange={setTrustOpen}
          onLabelChange={setTrustLabel}
          onImport={importTrust}
        />
      )}
      {downstreamIdentityOpen && (
        <ImportIdentityModal
          open
          busy={props.busy}
          label={downstreamIdentityLabel}
          password={downstreamIdentityPassword}
          title="导入本监听独立服务端身份"
          description="选择 .p12 / .pfx，或同时包含服务端证书链与匹配私钥的 .pem。本机代理接受 App/客户端 TLS 连接时出示此身份；它不是代理连接上游 Server 时使用的客户端身份。"
          detail="导入时校验证书与私钥匹配、有效期、DigitalSignature 和 serverAuth。文件路径与密码不会写入 Workspace；仅保存受系统保护的安全引用。"
          buttonLabel="选择服务端身份（.p12 / .pfx / .pem）"
          buttonAriaLabel="选择服务端身份（.p12 / .pfx / .pem）"
          passwordLabel="P12 / PFX 密码（PEM 不使用；允许为空）"
          onOpenChange={changeDownstreamIdentityOpen}
          onLabelChange={setDownstreamIdentityLabel}
          onPasswordChange={setDownstreamIdentityPassword}
          onImport={importDownstreamIdentity}
        />
      )}
      {downstreamTrustOpen && (
        <ImportPemModal
          open
          busy={props.busy}
          label={downstreamTrustLabel}
          title="导入用于验证客户端证书的 CA"
          description="仅在客户端证书模式为“可选”或“必须”时使用。请选择签发客户端证书的 CA，不要选择客户端自身的 client.p12 或本机代理服务端证书。"
          detail="导入时会校验 CA 能力并保存受保护引用；完成后会在当前页面显示主题、SAN、有效期和 SHA-256。"
          buttonLabel="选择客户端 CA（.cer / .crt / .pem / .der）"
          onOpenChange={setDownstreamTrustOpen}
          onLabelChange={setDownstreamTrustLabel}
          onImport={importDownstreamTrust}
        />
      )}
    </div>
  );
}
