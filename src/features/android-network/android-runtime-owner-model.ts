import type {
  AndroidRuntimeOwnerMode,
  AndroidRuntimeOwnerTransitionReason,
  AndroidRuntimeOwnerViewModel,
} from "@/generated/rust-types";

export type RuntimeOwnerDisplay = Pick<
  AndroidRuntimeOwnerViewModel,
  "serial" | "mode" | "state" | "transition_reason"
>;

export function runtimeOwnerQueryKey(
  owner: Pick<AndroidRuntimeOwnerViewModel, "serial" | "epoch"> | null | undefined,
): string | undefined {
  return owner ? `owner:${owner.serial}:${owner.epoch}` : undefined;
}

export function isForeignRuntimeOwner(
  selectedSerial: string | null | undefined,
  ownerSerial: string | null | undefined,
): boolean {
  return Boolean(ownerSerial && ownerSerial !== selectedSerial);
}

export function runtimeOwnerModeText(mode: AndroidRuntimeOwnerMode): string {
  if (mode === "adb_reverse") return "USB / ADB Reverse";
  if (mode === "lan") return "局域网";
  if (mode === "device_only") return "仅设备端";
  return assertNever(mode);
}

export function runtimeOwnerStateText(owner: RuntimeOwnerDisplay): string {
  if (
    owner.state === "waiting_reconnect"
    || owner.transition_reason === "device_disconnected"
  ) {
    return `设备已断开，等待同一序列号 ${owner.serial} 重连；仍可停止或紧急恢复`;
  }
  if (
    owner.state === "cleanup_required"
    || owner.transition_reason === "reverse_cleanup_required"
  ) {
    return `遗留连接需要清理；请让同一序列号 ${owner.serial} 重连后停止或紧急恢复`;
  }
  if (owner.state === "active") return "运行记录有效";
  if (owner.state === "uncertain") return "运行状态待确认，仍保留运行所有权";
  if (owner.state === "stop_failed") return "上次停止失败，仍需停止或紧急恢复";
  return assertNever(owner.state);
}

export function runtimeOwnerTransitionText(
  reason: AndroidRuntimeOwnerTransitionReason,
): string {
  if (reason === "activation_confirmed") return "启动已确认";
  if (reason === "activation_uncertain") return "启动结果待确认";
  if (reason === "reverse_preparation") return "ADB Reverse 准备中";
  if (reason === "reverse_cleanup_required") return "ADB Reverse 仍需清理";
  if (reason === "device_disconnected") return "设备已断开";
  if (reason === "device_reconnected") return "同序列号设备已重连";
  if (reason === "stop_failed") return "停止失败";
  if (reason === "recovered_from_storage") return "从本机恢复运行记录";
  return assertNever(reason);
}

function assertNever(value: never): never {
  throw new Error(`未处理的 Android runtime owner 状态：${String(value)}`);
}
