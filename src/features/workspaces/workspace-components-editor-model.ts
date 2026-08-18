import type {
  CertificateReferenceKind,
  ConnectionFaultAction,
} from "@/generated/rust-types";

export type ComponentKind =
  | "response_assertion"
  | "fault_preset"
  | "certificate_reference";

export type ComponentOperation = "delete" | "listener_ids" | "variant";

export const certificateKindLabels: Record<CertificateReferenceKind, string> = {
  mitm_root_ca: "MITM Root CA",
  reverse_server_identity: "Reverse 服务端身份",
  downstream_client_trust: "下游客户端信任",
  upstream_client_identity: "上游客户端身份",
  upstream_server_trust: "上游服务端信任",
};

export function updateAtIndex<T>(
  items: readonly T[],
  index: number,
  update: (item: T) => T,
) {
  return items.map((item, itemIndex) =>
    itemIndex === index ? update(item) : item,
  );
}

export function faultActionValue(action: ConnectionFaultAction) {
  switch (action.kind) {
    case "delay":
    case "idle_timeout":
      return action.milliseconds;
    case "rate_limit":
      return action.bytes_per_second;
    case "close_after_bytes":
    case "half_close_after_bytes":
      return action.bytes;
    case "reject":
      return 0;
  }
}

export function faultActionLabel(action: ConnectionFaultAction) {
  return action.kind === "delay" || action.kind === "idle_timeout"
    ? "毫秒"
    : action.kind === "rate_limit"
      ? "字节/秒"
      : "字节数";
}

export function updateFaultAction(
  action: ConnectionFaultAction,
  next: number,
) {
  switch (action.kind) {
    case "delay":
    case "idle_timeout":
      return { ...action, milliseconds: next };
    case "rate_limit":
      return { ...action, bytes_per_second: next };
    case "close_after_bytes":
    case "half_close_after_bytes":
      return { ...action, bytes: next };
    case "reject":
      return action;
  }
}
