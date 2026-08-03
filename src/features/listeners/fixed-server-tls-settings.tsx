"use client";

/**
 * 当前监听的双向 TLS 配置编排。
 *
 * 下游与上游是两次独立握手，因此分别展示“谁向谁证明身份”。证书文件选择、解析、
 * 密钥保护和真实握手全部由 Rust 执行；本组件只维护弹窗与用户输入状态。
 */

import { useState } from "react";
import type {
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamTlsTestViewModel,
  ProxyListener,
} from "@/generated/rust-types";
import { DownstreamTlsCard } from "./downstream-tls-card";
import {
  ImportIdentityModal,
  ImportTrustModal,
} from "./fixed-server-tls-import-modals";
import { UpstreamTlsCard } from "./upstream-tls-card";

type Props = {
  listener: ProxyListener;
  certificateReferences: CertificateReference[];
  certificateDetails: ListenerCertificateDetailViewModel[];
  busy: boolean;
  testing: boolean;
  testResult?: ListenerUpstreamTlsTestViewModel;
  testError?: string;
  onChange: (changes: Partial<ProxyListener>) => void;
  onImportClientIdentity: (label: string, password: string) => Promise<boolean>;
  onImportServerTrust: (label: string) => Promise<boolean>;
  onTest: () => Promise<void>;
};

export function FixedServerTlsSettings(props: Props) {
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

  return (
    <div className="col-span-2 space-y-4 max-[700px]:col-span-1">
      <DownstreamTlsCard
        listener={props.listener}
        certificateReferences={props.certificateReferences}
        certificateDetails={props.certificateDetails}
        onChange={props.onChange}
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
      <ImportIdentityModal
        open={identityOpen}
        busy={props.busy}
        label={identityLabel}
        password={identityPassword}
        onOpenChange={setIdentityOpen}
        onLabelChange={setIdentityLabel}
        onPasswordChange={setIdentityPassword}
        onImport={importIdentity}
      />
      <ImportTrustModal
        open={trustOpen}
        busy={props.busy}
        label={trustLabel}
        onOpenChange={setTrustOpen}
        onLabelChange={setTrustLabel}
        onImport={importTrust}
      />
    </div>
  );
}
