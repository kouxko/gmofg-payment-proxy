import { Alert, Button, Card, Chip } from "@heroui/react";
import { ArrowDownToLine } from "@gravity-ui/icons";
import type {
  CertificateItemViewModel,
  CertificateOverviewViewModel,
} from "@/generated/rust-types";
import { formatTimestamp, toneColor } from "@/lib/format";

export type CertificatePendingAction =
  | "generate"
  | "reissue"
  | "export"
  | "validate";

type CertificateOverviewSectionProps = {
  overview?: CertificateOverviewViewModel;
  localItems: CertificateItemViewModel[];
  leafSansAvailable: boolean;
  writePending: boolean;
  pendingAction?: CertificatePendingAction;
  onGenerate: () => void;
  onExport: () => void;
  onReissue: () => void;
  onOpenListeners: () => void;
};

export function CertificateOverviewSection({
  overview,
  localItems,
  leafSansAvailable,
  writePending,
  pendingAction,
  onGenerate,
  onExport,
  onReissue,
  onOpenListeners,
}: CertificateOverviewSectionProps) {
  const canChange = overview?.can_change ?? false;

  return (
    <div className="space-y-4">
      <Card>
        <Card.Header>
          <Card.Title>A. 本机 Root CA 与客户端 → Proxy 服务端身份</Card.Title>
        </Card.Header>
        <Card.Content className="space-y-4">
          <div
            data-testid="certificate-overview-grid"
            className="grid grid-cols-2 gap-x-6 gap-y-4 max-[960px]:grid-cols-1"
          >
            {localItems.map((item) => (
              <CertificateMetadata key={item.kind} item={item} />
            ))}
          </div>
          <div className="flex flex-wrap gap-3">
            {overview?.can_initialize && (
              <Button
                variant="primary"
                isDisabled={!canChange || !leafSansAvailable || writePending}
                onPress={onGenerate}
              >
                {pendingAction === "generate"
                  ? "正在生成…"
                  : "初始化本机证书"}
              </Button>
            )}
            <Button
              variant="outline"
              isDisabled={writePending}
              onPress={onExport}
            >
              <ArrowDownToLine className="size-4" />
              {pendingAction === "export" ? "正在导出…" : "导出公开 Root CA"}
            </Button>
            <Button
              variant="outline"
              isDisabled={!canChange || !leafSansAvailable || writePending}
              onPress={onReissue}
            >
              {pendingAction === "reissue"
                ? "正在重签…"
                : "重新签发服务端证书"}
            </Button>
          </div>
        </Card.Content>
      </Card>

      <Alert status="accent">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>Server TLS/mTLS 按代理监听配置</Alert.Title>
          <Alert.Description>
            每条启用固定 Server 的监听可以分别使用不同的 Server CA、主机名校验策略和可选
            PKCS12 客户端身份。请在对应监听中导入并执行真实握手测试。
          </Alert.Description>
        </Alert.Content>
        <Button variant="outline" onPress={onOpenListeners}>
          去配置代理入口
        </Button>
      </Alert>
    </div>
  );
}

function CertificateMetadata({ item }: { item: CertificateItemViewModel }) {
  return (
    <div className="min-w-0 space-y-2 border-b border-[var(--telemetry-line)] pb-4">
      <div className="flex flex-wrap items-center gap-2 font-semibold">
        <span className="min-w-0 break-words">{item.usage}</span>
        <Chip size="sm" color={toneColor(item.ui_tone)} variant="soft">
          {item.status_text}
        </Chip>
      </div>
      <dl className="grid min-w-0 grid-cols-[112px_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm max-[560px]:grid-cols-1 max-[560px]:gap-y-1">
        <dt>主题</dt>
        <dd className="min-w-0 break-words">{item.subject}</dd>
        <dt>SAN</dt>
        <dd className="min-w-0 break-words">
          {item.sans.join("、") || "—"}
        </dd>
        <dt>有效期</dt>
        <dd className="min-w-0 break-words">
          {formatTimestamp(item.valid_from)} ～ {formatTimestamp(item.valid_until)}
        </dd>
        <dt>SHA-256 指纹</dt>
        <dd className="min-w-0 break-all font-mono text-xs">
          {item.sha256_fingerprint}
        </dd>
      </dl>
    </div>
  );
}
