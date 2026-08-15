"use client";

import { Alert, Chip, Label, ListBox, Select } from "@heroui/react";
import type {
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ListenerUpstreamConnectionTestViewModel,
} from "@/generated/rust-types";
import { formatTimestamp, toneColor } from "@/lib/format";

const NONE = "__none__";

export function CertificateReferenceSelect({
  label,
  value,
  emptyLabel,
  references,
  isDisabled = false,
  onChange,
}: {
  label: string;
  value: string | null;
  emptyLabel: string;
  references: CertificateReference[];
  isDisabled?: boolean;
  onChange: (value: string | null) => void;
}) {
  return (
    <Select
      aria-label={label}
      selectedKey={value ?? NONE}
      isDisabled={isDisabled}
      onSelectionChange={(key) => {
        // HeroUI 清空选择时可能传入 null；锁定态或空选择都不能污染证书引用。
        if (key === null || isDisabled) return;
        onChange(String(key) === NONE ? null : String(key));
      }}
    >
      <Label>{label}</Label>
      <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
      <Select.Popover>
        <ListBox>
          <ListBox.Item id={NONE} textValue={emptyLabel}>{emptyLabel}</ListBox.Item>
          {references.map((reference) => (
            <ListBox.Item
              key={reference.id}
              id={reference.id}
              textValue={reference.label}
            >
              {reference.label}
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}

export function ConnectionTestResult({
  result,
  showTlsDetails = true,
}: {
  result: ListenerUpstreamConnectionTestViewModel;
  showTlsDetails?: boolean;
}) {
  return (
    <Alert status="success" className="col-span-2 max-[700px]:col-span-1">
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{result.message}</Alert.Title>
        <Alert.Description>
          <span className="grid gap-1">
            <span>连接：{result.resolved_address} · {result.elapsed_millis} ms</span>
            <span>传输：{result.transport}</span>
            {showTlsDetails && result.tls && (
              <>
                <span>协商：{result.tls.tls_version} · {result.tls.cipher_suite}</span>
                <span>Server：{result.tls.peer_subject}</span>
                <span className="break-all font-mono text-xs">
                  SHA-256：{result.tls.peer_sha256_fingerprint}
                </span>
                <span>
                  主机名校验：
                  {result.tls.hostname_verification_enabled
                    ? "已启用并通过"
                    : "按入口配置关闭"}
                  {" · "}客户端身份：
                  {result.tls.client_identity_configured
                    ? "已配置"
                    : "未配置（普通 TLS）"}
                </span>
              </>
            )}
          </span>
        </Alert.Description>
      </Alert.Content>
    </Alert>
  );
}

export function CertificateDetailPanel({
  reference,
  detail,
  emptyText,
}: {
  reference?: CertificateReference;
  detail?: ListenerCertificateDetailViewModel;
  emptyText: string;
}) {
  if (!reference) {
    return (
      <p className="rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3 text-sm text-[var(--telemetry-muted)]">
        {emptyText}
      </p>
    );
  }
  if (detail?.error_message) {
    return (
      <Alert status="danger">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{reference.label} 无法读取</Alert.Title>
          <Alert.Description>{detail.error_message}</Alert.Description>
        </Alert.Content>
      </Alert>
    );
  }
  const certificate = detail?.certificate;
  if (!certificate) {
    return (
      <p className="rounded-xl bg-[var(--telemetry-table-head)] px-4 py-3 text-sm text-[var(--telemetry-muted)]">
        正在读取“{reference.label}”的证书详情…
      </p>
    );
  }
  return (
    <div className="space-y-3 rounded-xl border border-[var(--telemetry-line)] bg-[var(--telemetry-table-head)]/45 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-semibold">{reference.label}</span>
        <Chip size="sm" color={toneColor(certificate.ui_tone)} variant="soft">
          {certificate.status_text}
        </Chip>
      </div>
      <dl className="grid min-w-0 grid-cols-[96px_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1 max-[560px]:gap-y-1">
        <dt className="text-[var(--telemetry-muted)]">用途</dt>
        <dd className="min-w-0 break-words">{certificate.usage}</dd>
        <dt className="text-[var(--telemetry-muted)]">主题</dt>
        <dd className="min-w-0 break-words">{certificate.subject}</dd>
        <dt className="text-[var(--telemetry-muted)]">SAN</dt>
        <dd className="min-w-0 break-words">{certificate.sans.join("、") || "—"}</dd>
        <dt className="text-[var(--telemetry-muted)]">有效期</dt>
        <dd className="min-w-0 break-words">
          {formatTimestamp(certificate.valid_from)} ～ {formatTimestamp(certificate.valid_until)}
        </dd>
        <dt className="text-[var(--telemetry-muted)]">SHA-256</dt>
        <dd className="min-w-0 break-all font-mono text-xs">
          {certificate.sha256_fingerprint}
        </dd>
      </dl>
    </div>
  );
}
