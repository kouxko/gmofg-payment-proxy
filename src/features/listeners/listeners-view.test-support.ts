import { vi } from "vitest";
import type { HttpListenerSettings, ProxyListener } from "@/generated/rust-types";
import { defaultSocketRuntimeLimits } from "./listener-data-plane";

export const navigationMocks = { navigate: vi.fn() };

export const bootstrap = {
  certificate: {
        items: [{
          kind: "local_root_ca",
          subject: "CN=Intercept Proxy Root CA",
          usage: "动态签发代理服务端证书",
          sans: [],
          valid_from: "2026-01-01T00:00:00Z",
          valid_until: "2036-01-01T00:00:00Z",
          sha256_fingerprint: "55:66:77:88",
          status_text: "有效",
          ui_tone: "positive",
        }, {
          kind: "proxy_leaf",
          subject: "CN=10.0.0.8",
          usage: "本机代理服务端身份",
          sans: ["IP:10.0.0.8"],
          valid_from: "2026-01-01T00:00:00Z",
          valid_until: "2028-01-01T00:00:00Z",
          sha256_fingerprint: "11:22:33:44",
          status_text: "有效",
          ui_tone: "positive",
        }],
  },
};

export const mocks = {
  workspaceList: vi.fn(), workspaceGet: vi.fn(), workspaceValidate: vi.fn(), workspaceSave: vi.fn(),
  listenerValidate: vi.fn(),
  listenerNew: vi.fn(), listenerCopy: vi.fn(), listenerSave: vi.fn(), listenerDelete: vi.fn(),
  listenerOverview: vi.fn(), listenerStart: vi.fn(), listenerStop: vi.fn(),
  listenerTestUpstreamConnection: vi.fn(), listenerImportUpstreamClientIdentity: vi.fn(), listenerImportUpstreamServerTrust: vi.fn(),
  listenerImportDownstreamServerIdentity: vi.fn(), listenerImportDownstreamClientTrust: vi.fn(),
  listenerCertificateOverview: vi.fn(), listenerCertificateDiscard: vi.fn(),
  listenerProtocolPackageCatalog: vi.fn(),
  workspaceSecretStoreBasic: vi.fn(),
};

export function dynamicListener(id = "listener-1", name = "默认代理监听", port = 8080) {
  return {
    id, name, enabled: false, bind_address: "127.0.0.1", port,
    allowed_client_cidrs: [],
    connect_timeout_ms: 30000, read_timeout_ms: 70000, write_timeout_ms: 70000,
    data_plane: {
      kind: "http" as const,
      settings: {
        authentication: { mode: "none" as const },
        mitm: { enabled: false, authority_allowlist: [], root_ca: null, maximum_cached_leaf_certificates: 256 },
        downstream_tls: {
          enabled: false,
          server_identity: null,
          dynamic_sni_allowlist: [],
          client_authentication: { mode: "disabled" as const },
        },
        request_body_codec: "auto" as const,
        response_body_codec: "auto" as const,
        body_processing: { mode: "plain" as const },
        fixed_server: null,
      },
    },
  };
}

export function fixedListener(id: string, name: string, port: number, upstreamUrl: string) {
  return {
    ...dynamicListener(id, name, port),
    data_plane: {
      kind: "http" as const,
      settings: {
        ...dynamicListener(id, name, port).data_plane.settings,
        fixed_server: {
          upstream_url: upstreamUrl,
          upstream_tls: { verify_hostname: true, server_trust: null, client_identity: null },
        },
      },
    },
  };
}

export function withHttpSettings(
  listener: ProxyListener,
  changes: Partial<HttpListenerSettings>,
): ProxyListener {
  if (listener.data_plane.kind !== "http") throw new Error("expected HTTP listener");
  return {
    ...listener,
    data_plane: {
      kind: "http" as const,
      settings: { ...listener.data_plane.settings, ...changes },
    },
  };
}

export function socketListener(
  id = "socket-1",
  name = "Socket Relay",
  port = 9000,
  mode: "transparent" | "tcp_to_tls" | "tls_to_tcp" | "tls_to_tls" = "transparent",
) {
  const upstreamTls = { verify_hostname: true, tls_server_name: null, server_trust: null, client_identity: null };
  const downstreamTls = {
    server_identity: "",
    client_authentication: { mode: "disabled" as const },
  };
  const security = mode === "tcp_to_tls"
    ? { mode, upstream_tls: upstreamTls }
    : mode === "tls_to_tcp"
      ? { mode, downstream_tls: downstreamTls }
      : mode === "tls_to_tls"
        ? { mode, downstream_tls: downstreamTls, upstream_tls: upstreamTls }
        : { mode: "transparent" as const };
  return {
    ...dynamicListener(id, name, port),
    data_plane: {
      kind: "socket" as const,
      settings: {
        topology: {
          mode: "relay" as const,
          settings: {
            upstream: { host: "server.test", port: 9443 },
            security,
          },
        },
        maximum_connections: 500,
        runtime_limits: defaultSocketRuntimeLimits(),
        processing: { mode: "direct" as const },
      },
    },
  };
}

export function localResponderListener(
  id = "local-responder-1",
  name = "LocalResponder",
  port = 9001,
) {
  return {
    ...dynamicListener(id, name, port),
    data_plane: {
      kind: "socket" as const,
      settings: {
        topology: {
          mode: "local_responder" as const,
          settings: { downstream_security: { mode: "tcp" as const } },
        },
        maximum_connections: 100,
        runtime_limits: defaultSocketRuntimeLimits(),
        processing: {
          mode: "scripted" as const,
          settings: {
            package: { id: "example.local-responder", version: "1.0.0" },
          },
        },
      },
    },
  };
}

export const workspace = {
  id: "workspace-1", name: "API Lab", revision: 1,
  listeners: [dynamicListener()],
  metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [],
};

export function certificateReference(id: string, label: string, kind: "reverse_server_identity" | "downstream_client_trust" | "upstream_client_identity" | "upstream_server_trust") {
  return { id, label, kind, reference: `managed:${id}` };
}

export function certificateDetail(reference: ReturnType<typeof certificateReference>, subject = "CN=测试证书") {
  return {
    reference_id: reference.id,
    label: reference.label,
    certificate: {
      kind: reference.kind,
      subject,
      usage: reference.kind === "upstream_client_identity" ? "代理向上游服务器出示的 mTLS 客户端身份" : "验证上游服务器证书的 CA",
      sans: ["DNS:server.test", "IP:10.0.0.8"],
      valid_from: "2026-01-01T00:00:00Z",
      valid_until: "2028-01-01T00:00:00Z",
      sha256_fingerprint: "AA:BB:CC:DD",
      status_text: "有效",
      ui_tone: "positive" as const,
    },
    error_message: null,
  };
}

export function ok<T>(data: T) { return Promise.resolve({ status: "ok" as const, data }); }

export function commandError(message: string, fieldErrors: Record<string, string[]> = {}) {
  return Promise.resolve({
    status: "error" as const,
    error: {
      code: "LISTENER_OVERVIEW_FAILED",
      message,
      field_errors: fieldErrors,
      retryable: true,
      suggested_action: "请重试。",
    },
  });
}

export function listenerStatus(
  listenerId: string,
  state: "stopped" | "running" | "starting" | "stopping" | "faulted" = "stopped",
  capabilities?: { canStart: boolean; canStop: boolean },
) {
  const canStart = capabilities?.canStart ?? state === "stopped";
  const canStop = capabilities?.canStop ?? state !== "stopped";
  return {
    listener_id: listenerId,
    name: listenerId,
    kind_text: "正向代理",
    state,
    state_text: state === "running"
      ? "运行中"
      : state === "starting"
        ? "启动中"
        : state === "stopping"
          ? "停止中"
          : state === "faulted"
            ? "故障"
            : "已停止",
    ui_tone: state === "faulted" ? "danger" : state === "running" ? "positive" : "neutral",
    listen_address: "127.0.0.1:8080",
    request_destination: "请求中的目标地址",
    fault_reason: state === "faulted" ? "Listener 任务已意外结束。" : null,
    can_start: canStart,
    can_stop: canStop,
    active_connections: state === "running" ? 2 : 0,
    client_to_server_bytes: state === "running" ? 1024 : 0,
    server_to_client_bytes: state === "running" ? 2048 : 0,
  };
}

export function listenerOverview(rows = [listenerStatus("listener-1")]) {
  return {
    workspace_id: "workspace-1",
    workspace_name: "API Lab",
    state_text: rows.some((row) => row.state === "faulted")
      ? "部分入口故障"
      : rows.some((row) => row.state === "running")
        ? "部分入口运行中"
        : "全部入口已停止",
    ui_tone: rows.some((row) => row.state === "faulted")
      ? "danger" as const
      : rows.some((row) => row.state === "running")
        ? "warning" as const
        : "neutral" as const,
    total_count: rows.length,
    active_count: rows.filter((row) => row.state === "running").length,
    faulted_count: rows.filter((row) => row.state === "faulted").length,
    rows,
  };
}

export function setupListenerMocks() {
    vi.clearAllMocks();
    mocks.workspaceList.mockReturnValue(ok([{ id: "workspace-1", name: "API Lab", revision: 1, listener_count: 1, enabled_listener_count: 0, selected: true }]));
    mocks.workspaceGet.mockReturnValue(ok(workspace));
    mocks.workspaceValidate.mockImplementation((draft) => ok({ valid: true, normalized: draft, field_errors: {} }));
    mocks.listenerValidate.mockImplementation((workspaceId, revision, listener, certificateReferences) => ok({
      valid: true,
      normalized: {
        ...workspace,
        id: workspaceId,
        revision,
        listeners: [...workspace.listeners.filter((item) => item.id !== listener.id), listener],
        certificate_references: certificateReferences,
      },
      field_errors: {},
    }));
    mocks.workspaceSave.mockImplementation((draft) => ok({ ...draft, revision: 2 }));
    mocks.listenerSave.mockImplementation((_workspaceId, _revision, listener, certificateReferences) => ok({
      ...workspace,
      revision: 2,
      listeners: [...workspace.listeners.filter((item) => item.id !== listener.id), listener],
      certificate_references: certificateReferences,
    }));
    mocks.listenerDelete.mockReturnValue(ok({ success: true, cancelled: false, message: "Listener 已删除。", ui_tone: "positive", entity_id: null, revision: 2, requires_restart: false }));
    mocks.listenerNew.mockReturnValue(ok(dynamicListener("listener-new", "新建代理监听", 8081)));
    mocks.listenerCopy.mockImplementation((source) => ok({ ...source, id: "listener-copy", name: `${source.name} 副本`, enabled: false }));
    // 页面现在对缺失 runtime 行的持久化 Listener fail-closed 锁定。通用 fixture
    // 覆盖各测试文件常用 id，避免与测试目标无关的“未知状态”误锁表单。
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      "listener-1",
      "fixed-1",
      "socket-1",
      "local-responder-1",
      "listener-a",
      "listener-b",
      "transaction",
      "dll",
    ].map((id) => listenerStatus(id)))));
    mocks.listenerStart.mockImplementation((_workspaceId, _revision, listenerId) => ok(listenerStatus(listenerId, "running")));
    mocks.listenerStop.mockImplementation((_workspaceId, _revision, listenerId) => ok(listenerStatus(listenerId, "stopped")));
    mocks.listenerTestUpstreamConnection.mockReturnValue(ok({
      listener_id: "fixed-1", data_plane: "http", upstream_origin: "https://127.0.0.1:9443", resolved_address: "127.0.0.1:9443",
      scheme: "https", transport: "TCP + TLS", tls: {
        tls_version: "TLS 1.2", cipher_suite: "TLS_TEST", peer_subject: "CN=测试上游", peer_sha256_fingerprint: "AA:BB",
        hostname_verification_enabled: true, client_identity_configured: true,
      }, socket_transport_mode: null, elapsed_millis: 12,
      message: "上游 Server TLS 握手成功。", ui_tone: "positive",
    }));
    const identity = certificateReference("identity-ref-1", "测试身份", "upstream_client_identity");
    const trust = certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust");
    const downstreamIdentity = certificateReference("downstream-identity-ref-1", "入口服务端身份", "reverse_server_identity");
    const downstreamTrust = certificateReference("downstream-ca-ref-1", "终端客户端 CA", "downstream_client_trust");
    mocks.listenerImportUpstreamClientIdentity.mockReturnValue(ok({ reference: identity, detail: certificateDetail(identity, "CN=测试客户端身份") }));
    mocks.listenerImportUpstreamServerTrust.mockReturnValue(ok({ reference: trust, detail: certificateDetail(trust, "CN=测试上游 CA") }));
    mocks.listenerImportDownstreamServerIdentity.mockReturnValue(ok({ reference: downstreamIdentity, detail: certificateDetail(downstreamIdentity, "CN=proxy.test") }));
    mocks.listenerImportDownstreamClientTrust.mockReturnValue(ok({ reference: downstreamTrust, detail: certificateDetail(downstreamTrust, "CN=终端客户端 CA") }));
    mocks.listenerCertificateOverview.mockReturnValue(ok([]));
    mocks.listenerProtocolPackageCatalog.mockReturnValue(ok({
      options: [],
      installed_version_count: 0,
      unavailable_version_count: 0,
      recommended_package: null,
    }));
    mocks.listenerCertificateDiscard.mockReturnValue(ok({
      success: true,
      cancelled: false,
      message: "已清理未保存的证书材料。",
      ui_tone: "positive",
      entity_id: null,
      revision: null,
      requires_restart: false,
    }));
    mocks.workspaceSecretStoreBasic.mockReturnValue(ok({ provider: "system", key: "secret-ref-1" }));
}
