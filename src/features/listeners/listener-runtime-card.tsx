import { Button } from "@heroui/react";
import type { ListenerMonitorRowViewModel } from "@/generated/rust-types";
import { formatBytes } from "@/lib/format";

export type ListenerPending = "validate" | "save" | "delete" | "start" | "stop" | "secret" | "tls-test"
  | "import-downstream-identity" | "import-downstream-trust"
  | "import-upstream-identity" | "import-upstream-trust";

export function ListenerRuntimeCard({ status, isLoading, error, pending, onToggle, onRetry }: {
  status?: ListenerMonitorRowViewModel;
  isLoading: boolean;
  error?: string;
  pending?: ListenerPending;
  onToggle: () => Promise<void>;
  onRetry: () => Promise<void>;
}) {
  const unavailable = isLoading || Boolean(error) || !status;
  const operation = status?.can_stop ? "stop" : status?.can_start ? "start" : undefined;
  const stateText = isLoading ? "正在读取…" : error ? "查询失败"
    : status?.state_text ?? "未知（当前监听状态不可用）";
  const actionText = pending === "start" ? "启动中…" : pending === "stop" ? "停止中…"
    : unavailable ? "状态不可用" : operation === "stop" ? "停止监听"
      : operation === "start" ? "启动监听" : "无可用操作";
  return <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--telemetry-line)] p-3">
    <div className="min-w-0">
      <p className="text-sm">运行状态：{stateText}</p>
      {status && <p className="mt-1 text-xs text-[var(--telemetry-muted)]">{status.kind_text}</p>}
      {status && <p className="mt-1 text-xs text-[var(--telemetry-muted)]">
        活动连接 {status.active_connections} · C→S {formatBytes(status.client_to_server_bytes)}
        {" · "}S→C {formatBytes(status.server_to_client_bytes)}
      </p>}
      {error && <p className="mt-1 text-xs text-[var(--telemetry-danger)]">{error}</p>}
    </div>
    <div className="flex items-center gap-2">
      {error && <Button size="sm" variant="outline" onPress={() => void onRetry()}>重试状态查询</Button>}
      <Button variant={operation === "stop" ? "danger-soft" : "primary"}
        isDisabled={Boolean(pending) || unavailable || !operation} onPress={() => void onToggle()}>
        {actionText}
      </Button>
    </div>
  </div>;
}
