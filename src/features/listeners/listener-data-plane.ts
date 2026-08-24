import type {
  HttpListenerSettings,
  ListenerDataPlane,
  ProxyListener,
  SocketDownstreamTlsSettings,
  SocketRelaySecurity,
  SocketRelaySettings,
  SocketRuntimeLimits,
  SocketUpstreamTlsSettings,
} from "@/generated/rust-types";

export function defaultHttpDataPlane(): ListenerDataPlane {
  return {
    kind: "http",
    settings: {
      authentication: { mode: "none" },
      mitm: {
        enabled: false,
        authority_allowlist: [],
        root_ca: null,
        maximum_cached_leaf_certificates: 256,
      },
      downstream_tls: {
        enabled: false,
        server_identity: null,
        dynamic_sni_allowlist: [],
        client_authentication: { mode: "disabled" },
      },
      request_body_codec: "auto",
      response_body_codec: "auto",
      body_processing: { mode: "plain" },
      fixed_server: null,
    },
  };
}

export function defaultSocketDataPlane(): ListenerDataPlane {
  return {
    kind: "socket",
    settings: {
      topology: {
        mode: "relay",
        settings: {
          upstream: { host: "", port: 0 },
          security: { mode: "transparent" },
        },
      },
      maximum_connections: 500,
      runtime_limits: defaultSocketRuntimeLimits(),
      processing: { mode: "direct" },
    },
  };
}

/** 与 Rust `SocketRuntimeLimits::default()` 完全一致的显式运行时资源配置。 */
export function defaultSocketRuntimeLimits(): SocketRuntimeLimits {
  return {
    read_chunk_bytes: 16 * 1024,
    diagnostic_event_capacity: 256,
    diagnostic_memory_bytes: 1024 * 1024,
  };
}

export function changeDataPlaneKind(
  listener: ProxyListener,
  kind: ListenerDataPlane["kind"],
): Partial<ProxyListener> {
  if (listener.data_plane.kind === kind) return {};
  return {
    data_plane: kind === "http" ? defaultHttpDataPlane() : defaultSocketDataPlane(),
  };
}

export function changeHttpSettings(
  settings: HttpListenerSettings,
  changes: Partial<HttpListenerSettings>,
): Partial<ProxyListener> {
  return { data_plane: { kind: "http", settings: { ...settings, ...changes } } };
}

export function changeSocketSettings(
  settings: SocketRelaySettings,
  changes: Partial<SocketRelaySettings>,
): Partial<ProxyListener> {
  return { data_plane: { kind: "socket", settings: { ...settings, ...changes } } };
}

export function socketDownstreamTls(
  security: SocketRelaySecurity,
): SocketDownstreamTlsSettings | undefined {
  return security.mode === "tls_to_tcp" || security.mode === "tls_to_tls"
    ? security.downstream_tls
    : undefined;
}

export function socketUpstreamTls(
  security: SocketRelaySecurity,
): SocketUpstreamTlsSettings | undefined {
  return security.mode === "tcp_to_tls" || security.mode === "tls_to_tls"
    ? security.upstream_tls
    : undefined;
}
