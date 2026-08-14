import type {
  HttpListenerSettings,
  ListenerDataPlane,
  ProxyListener,
  SocketDownstreamTlsSettings,
  SocketRelaySecurity,
  SocketRelaySettings,
  SocketRelayTopology,
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
      processing: { mode: "direct" },
    },
  };
}

/**
 * Socket 的网络字段属于 Relay topology，LocalResponder 从类型上就没有上游。
 * 所有调用方必须先通过这个窄化函数，再读取 upstream/security，避免用空地址模拟无上游。
 */
export function socketRelayTopology(
  settings: SocketRelaySettings,
): SocketRelayTopology | undefined {
  return settings.topology.mode === "relay" ? settings.topology.settings : undefined;
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

export function changeSocketSecurity(
  mode: SocketRelaySecurity["mode"],
  current?: SocketRelaySecurity,
): SocketRelaySecurity {
  const downstream = current
    ? socketDownstreamTls(current) ?? defaultSocketDownstreamTls()
    : defaultSocketDownstreamTls();
  const upstream = current
    ? socketUpstreamTls(current) ?? defaultSocketUpstreamTls()
    : defaultSocketUpstreamTls();
  if (mode === "tcp_to_tls") return { mode, upstream_tls: upstream };
  if (mode === "tls_to_tcp") return { mode, downstream_tls: downstream };
  if (mode === "tls_to_tls") {
    return { mode, downstream_tls: downstream, upstream_tls: upstream };
  }
  return { mode: "transparent" };
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

function defaultSocketDownstreamTls(): SocketDownstreamTlsSettings {
  return {
    server_identity: "",
    client_authentication: { mode: "disabled" },
  };
}

function defaultSocketUpstreamTls(): SocketUpstreamTlsSettings {
  return {
    verify_hostname: true,
    server_trust: null,
    client_identity: null,
  };
}
