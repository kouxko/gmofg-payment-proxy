import type { ListenerDraft } from "@/generated/rust-types";
import { socketWorkingMode } from "./socket-listener-model";

export function isLocalResponder(listener?: ListenerDraft): boolean {
  return listener?.data_plane.kind === "socket"
    && listener.data_plane.settings.topology.mode === "local_responder";
}

export function hasUpstream(listener: ListenerDraft) {
  if (listener.data_plane.kind === "socket") {
    return listener.data_plane.settings.topology.mode === "relay";
  }
  return listener.data_plane.settings.topology.mode === "remote_server"
    && listener.data_plane.settings.topology.settings.fixed_server !== null;
}

export function listenerKind(listener: ListenerDraft) {
  if (listener.data_plane.kind === "socket") {
    const mode = socketWorkingMode(listener.data_plane.settings);
    if (mode === "raw_relay") return "Socket · 透明转发";
    if (mode === "protocol_relay") return "Socket · 按协议转发";
    if (mode === "local_response") return "Socket · 本地应答";
    return "Socket · 配置不兼容";
  }
  if (listener.data_plane.settings.topology.mode === "local_server") return "HTTP · LocalServer";
  return listener.data_plane.settings.topology.settings.fixed_server
    ? "HTTP · 固定 Server"
    : "HTTP · 按请求目标";
}

export function isFieldErrorRecord(value: unknown): value is Record<string, string[]> {
  return typeof value === "object"
    && value !== null
    && !Array.isArray(value)
    && Object.entries(value).every(([field, messages]) => field.length > 0
      && Array.isArray(messages)
      && messages.every((message) => typeof message === "string"));
}
