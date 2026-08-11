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
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
  ProxyListener,
} from "@/generated/rust-types";
import { DownstreamTlsCard } from "./downstream-tls-card";
import {
  ImportIdentityModal,
  ImportPemModal,
  ImportTrustModal,
} from "./fixed-server-tls-import-modals";
import { UpstreamTlsCard } from "./upstream-tls-card";

type Props = {
  listener: ProxyListener;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  installationRoot?: CertificateItemViewModel;
  busy: boolean;
  testing: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
  onChange: (changes: Partial<ProxyListener>) => void;
  onImportDownstreamServerIdentity: (label: string) => Promise<boolean>;
  onImportDownstreamClientTrust: (label: string) => Promise<boolean>;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTest: () => Promise<void>;
};

export function FixedServerTlsSettings(props: Props) {
  const [downstreamIdentityOpen, setDownstreamIdentityOpen] = useState(false);
  const [downstreamIdentityLabel, setDownstreamIdentityLabel] = useState("本监听独立服务端身份");
  const [downstreamTrustOpen, setDownstreamTrustOpen] = useState(false);
  const [downstreamTrustLabel, setDownstreamTrustLabel] = useState("客户端证书 CA");
  const [identityOpen, setIdentityOpen] = useState(false);
  const [identityLabel, setIdentityLabel] = useState("上游 mTLS 客户端身份");
  const [identityPassword, setIdentityPassword] = useState("");
  const [trustOpen, setTrustOpen] = useState(false);
  const [trustLabel, setTrustLabel] = useState("上游服务器 CA");

  async function importIdentity() {
    if (!(await props.onImportClientIdentity(identityLabel, identityPassword))) return;
    setIdentityPassword("");
    setIdentityOpen(false);
  }

  async function importTrust() {
    if (!(await props.onImportServerTrust(trustLabel))) return;
    setTrustOpen(false);
  }

  async function importDownstreamIdentity() {
    if (!(await props.onImportDownstreamServerIdentity(downstreamIdentityLabel))) return;
    setDownstreamIdentityOpen(false);
  }

  async function importDownstreamTrust() {
    if (!(await props.onImportDownstreamClientTrust(downstreamTrustLabel))) return;
    setDownstreamTrustOpen(false);
  }

  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      <DownstreamTlsCard
        listener={props.listener}
        certificateReferences={props.certificateReferences}
        certificateDetails={props.certificateDetails}
        installationRoot={props.installationRoot}
        busy={props.busy}
        onChange={props.onChange}
        onOpenIdentityImport={() => setDownstreamIdentityOpen(true)}
        onOpenTrustImport={() => setDownstreamTrustOpen(true)}
      />
      <UpstreamTlsCard
        listener={props.listener}
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
          onOpenChange={setIdentityOpen}
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
        <ImportPemModal
          open
          busy={props.busy}
          label={downstreamIdentityLabel}
          title="导入本监听独立服务端身份"
          description="选择同时包含服务端证书链与对应私钥的 PEM 文件。本机代理接受客户端 TLS 连接时会出示该身份。一般监听直接使用证书管理页签发的本机叶子证书，无需重复导入。"
          detail="Rust 会校验证书与私钥匹配、有效期、DigitalSignature 和 serverAuth，并将材料保存为受系统密钥保护的引用；原文件路径不会写入 Workspace。"
          buttonLabel="选择服务端身份 PEM"
          onOpenChange={setDownstreamIdentityOpen}
          onLabelChange={setDownstreamIdentityLabel}
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
          detail="Rust 会校验 CA 能力并保存受保护引用；导入后会在当前页面显示主题、SAN、有效期和 SHA-256。"
          buttonLabel="选择客户端 CA（.cer / .crt / .pem / .der）"
          onOpenChange={setDownstreamTrustOpen}
          onLabelChange={setDownstreamTrustLabel}
          onImport={importDownstreamTrust}
        />
      )}
    </div>
  );
}
