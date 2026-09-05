import type {
  FixedServerSettings,
  HttpListenerDraftSettings,
  ListenerDraft,
  SocketDownstreamTlsDraftSettings,
  SocketRelayDraftSettings,
  SocketRelaySecurityDraft,
  SocketUpstreamTlsSettings,
} from "@/generated/rust-types";

export function httpFixedServer(
  settings: HttpListenerDraftSettings,
): FixedServerSettings | null {
  return settings.topology.mode === "remote_server"
    ? settings.topology.settings.fixed_server
    : null;
}

export function changeHttpSettings(
  settings: HttpListenerDraftSettings,
  changes: Partial<HttpListenerDraftSettings>,
): Partial<ListenerDraft> {
  return { data_plane: { kind: "http", settings: { ...settings, ...changes } } };
}

export function changeSocketSettings(
  settings: SocketRelayDraftSettings,
  changes: Partial<SocketRelayDraftSettings>,
): Partial<ListenerDraft> {
  return { data_plane: { kind: "socket", settings: { ...settings, ...changes } } };
}

export function socketDownstreamTls(
  security: SocketRelaySecurityDraft,
): SocketDownstreamTlsDraftSettings | undefined {
  return security.mode === "tls_to_tcp" || security.mode === "tls_to_tls"
    ? security.downstream_tls
    : undefined;
}

export function socketUpstreamTls(
  security: SocketRelaySecurityDraft,
): SocketUpstreamTlsSettings | undefined {
  return security.mode === "tcp_to_tls" || security.mode === "tls_to_tls"
    ? security.upstream_tls
    : undefined;
}
