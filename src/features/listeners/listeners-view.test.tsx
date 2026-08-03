// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ListenersView } from "./listeners-view";

const navigationMocks = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));

const mocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), workspaceValidate: vi.fn(), workspaceSave: vi.fn(),
  listenerNew: vi.fn(), listenerCopy: vi.fn(), listenerStatuses: vi.fn(), listenerStart: vi.fn(), listenerStop: vi.fn(),
  listenerTestUpstreamTls: vi.fn(), listenerImportUpstreamClientIdentity: vi.fn(), listenerImportUpstreamServerTrust: vi.fn(),
  listenerCertificateOverview: vi.fn(),
  workspaceSecretStoreBasic: vi.fn(),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

function dynamicListener(id = "listener-1", name = "默认代理监听", port = 8080) {
  return {
    id, name, enabled: false, bind_address: "127.0.0.1", port,
    authentication: { mode: "none" as const }, allowed_client_cidrs: [],
    mitm: { enabled: false, authority_allowlist: [], root_ca: null, maximum_cached_leaf_certificates: 256 },
    connect_timeout_ms: 30000, read_timeout_ms: 70000, write_timeout_ms: 70000,
    downstream_tls: { enabled: false, server_identity: null, client_authentication: { mode: "disabled" as const } },
    request_body_codec: "raw" as const,
    response_body_codec: "raw" as const,
    fixed_server: null,
  };
}

function fixedListener(id: string, name: string, port: number, upstreamUrl: string) {
  return {
    ...dynamicListener(id, name, port),
    fixed_server: {
      upstream_url: upstreamUrl,
      upstream_tls: { verify_hostname: true, server_trust: null, client_identity: null },
    },
  };
}

const workspace = {
  id: "workspace-1", name: "API Lab", revision: 1,
  listeners: [dynamicListener()],
  metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [],
};

function certificateReference(id: string, label: string, kind: "reverse_server_identity" | "downstream_client_trust" | "upstream_client_identity" | "upstream_server_trust") {
  return { id, label, kind, reference: `managed:${id}` };
}

function certificateDetail(reference: ReturnType<typeof certificateReference>, subject = "CN=测试证书") {
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

function ok<T>(data: T) { return Promise.resolve({ status: "ok" as const, data }); }

function listenerStatus(listenerId: string, state: "stopped" | "running" | "starting" = "stopped") {
  return {
    listener_id: listenerId,
    state,
    state_text: state === "running" ? "运行中" : state === "starting" ? "启动中" : "已停止",
    ui_tone: state === "running" ? "positive" : "neutral",
    listen_address: "127.0.0.1:8080",
    fault_reason: null,
    can_start: state === "stopped",
    can_stop: state !== "stopped",
  };
}

describe("统一代理监听编辑器", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.workspaceList.mockReturnValue(ok([{ id: "workspace-1", name: "API Lab", revision: 1, listener_count: 1, enabled_listener_count: 0, selected: true }]));
    mocks.workspaceGet.mockReturnValue(ok(workspace));
    mocks.workspaceValidate.mockImplementation((draft) => ok({ valid: true, normalized: draft, field_errors: {} }));
    mocks.workspaceSave.mockImplementation((draft) => ok({ ...draft, revision: 2 }));
    mocks.listenerNew.mockReturnValue(ok(dynamicListener("listener-new", "新建代理监听", 8081)));
    mocks.listenerCopy.mockImplementation((source) => ok({ ...source, id: "listener-copy", name: `${source.name} 副本`, enabled: false }));
    mocks.listenerStatuses.mockReturnValue(ok([]));
    mocks.listenerStart.mockImplementation((_workspaceId, _revision, listenerId) => ok(listenerStatus(listenerId, "running")));
    mocks.listenerStop.mockImplementation((_workspaceId, _revision, listenerId) => ok(listenerStatus(listenerId, "stopped")));
    mocks.listenerTestUpstreamTls.mockReturnValue(ok({
      listener_id: "fixed-1", upstream_origin: "https://127.0.0.1:9443", resolved_address: "127.0.0.1:9443",
      tls_version: "TLS 1.2", cipher_suite: "TLS_TEST", peer_subject: "CN=测试上游", peer_sha256_fingerprint: "AA:BB",
      hostname_verification_enabled: true, client_identity_configured: true, elapsed_millis: 12,
      message: "上游 Server TLS 握手成功。", ui_tone: "positive",
    }));
    const identity = certificateReference("identity-ref-1", "测试身份", "upstream_client_identity");
    const trust = certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust");
    mocks.listenerImportUpstreamClientIdentity.mockReturnValue(ok({ reference: identity, detail: certificateDetail(identity, "CN=测试客户端身份") }));
    mocks.listenerImportUpstreamServerTrust.mockReturnValue(ok({ reference: trust, detail: certificateDetail(trust, "CN=测试上游 CA") }));
    mocks.listenerCertificateOverview.mockReturnValue(ok([]));
    mocks.workspaceSecretStoreBasic.mockReturnValue(ok({ provider: "system", key: "secret-ref-1" }));
  });

  it("只提供一个新建入口并调用无参数 Rust command", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    expect(await screen.findByRole("button", { name: "新建代理监听" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /新增正向|新增转发/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "新建代理监听" }));
    expect(mocks.listenerNew).toHaveBeenCalledWith();
    expect(await screen.findByRole("textbox", { name: "代理监听名称" })).toHaveValue("新建代理监听");
  });

  it("默认按请求目标转发，并可在同一监听启用固定 Server", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    expect(await screen.findByText("请求转发方式")).toBeVisible();
    expect(screen.getByText("按原请求目标转发")).toBeVisible();
    expect(screen.getByText(/读取每个请求中的目标主机和端口/)).toBeVisible();
    expect(screen.getByRole("switch", { name: "为此监听启用 TLS" })).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "固定 Server URL" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "转发到固定 Server" }));
    expect(await screen.findByRole("textbox", { name: "固定 Server URL" })).toBeVisible();
    expect(screen.getByText("固定 Server 目标")).toBeVisible();
    expect(screen.getByText(/忽略请求中的目标地址/)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "允许的客户端 CIDR" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用 HTTP Basic 认证" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "为此监听启用 TLS" })).toBeVisible();
    expect(screen.queryByRole("switch", { name: "启用 allowlist MITM" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "校验上游服务器主机名" })).toBeChecked();
    expect(screen.getByRole("button", { name: "导入 Server CA" })).toBeVisible();
    expect(screen.getByRole("button", { name: "导入 client.p12" })).toBeVisible();
    expect(screen.queryByRole("textbox", { name: /Body Codec 引用/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /请求正文编码/ })).toBeVisible();
    expect(screen.getByRole("button", { name: /响应正文编码/ })).toBeVisible();
    await user.click(screen.getByRole("switch", { name: "转发到固定 Server" }));
    expect(screen.queryByRole("textbox", { name: "固定 Server URL" })).not.toBeInTheDocument();
    expect(screen.getByText("按原请求目标转发")).toBeVisible();
  });

  it("固定 Server 关闭时保留 Basic、CIDR 与 MITM 设置", async () => {
    render(<ListenersView />);
    expect(await screen.findByRole("textbox", { name: "允许的客户端 CIDR" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用 HTTP Basic 认证" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用 allowlist MITM" })).toBeVisible();
  });

  it("由 Rust 校验后保存统一监听", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "本地代理");
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    expect(mocks.workspaceSave.mock.calls[0][0].listeners[0].name).toBe("本地代理");
  });

  it("配置未修改时可在其他监听运行中直接启动第二个监听", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待启动监听", 8081),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    mocks.listenerStatuses.mockReturnValue(ok([listenerStatus("running-1", "running"), listenerStatus("stopped-2")]));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待启动监听"));
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 1, "stopped-2"));
    expect(mocks.workspaceValidate).not.toHaveBeenCalled();
    expect(mocks.workspaceSave).not.toHaveBeenCalled();
  });

  it("其他监听运行时阻止保存脏草稿并明确提示", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待启动监听", 8081),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    mocks.listenerStatuses.mockReturnValue(ok([listenerStatus("running-1", "running"), listenerStatus("stopped-2")]));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待启动监听"));
    const name = screen.getByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "修改后的监听");
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    expect(await screen.findByText(/已有其他监听正在运行/)).toBeVisible();
    expect(mocks.workspaceSave).not.toHaveBeenCalled();
    expect(mocks.listenerStart).not.toHaveBeenCalled();
  });

  it("修改后恢复为持久化值时视为无未保存差异", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待启动监听", 8081),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    mocks.listenerStatuses.mockReturnValue(ok([listenerStatus("running-1", "running"), listenerStatus("stopped-2")]));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待启动监听"));
    const name = screen.getByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "临时名称");
    await user.clear(name); await user.type(name, "待启动监听");
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 1, "stopped-2"));
    expect(mocks.workspaceSave).not.toHaveBeenCalled();
  });

  it("没有其他运行监听时仍先保存脏草稿再启动", async () => {
    const user = userEvent.setup(); render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "修改后的监听");
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 2, "listener-1");
  });

  it("固定 Server 的 TLS 测试先保存同一监听快照", async () => {
    const fixedWorkspace = { ...workspace, listeners: [fixedListener("fixed-1", "交易", 16627, "https://127.0.0.1:9443")] };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "测试上游 TLS / mTLS 握手" }));
    await waitFor(() => expect(mocks.workspaceValidate).toHaveBeenCalledWith(fixedWorkspace));
    expect(mocks.listenerTestUpstreamTls).toHaveBeenCalledWith("workspace-1", "fixed-1");
    expect(await screen.findByText(/127.0.0.1:9443 · 12 ms/)).toBeVisible();
  });

  it("导入 CA 后只把安全引用绑定到当前固定 Server", async () => {
    const fixedWorkspace = { ...workspace, listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")] };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    expect(screen.getByText(/签发上游 Server 证书的 ca\.crt/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.crt / .pem）" }));
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    expect(mocks.workspaceSave.mock.calls[0][0].listeners[0].fixed_server.upstream_tls.server_trust).toBe("ca-ref-1");
    expect(await screen.findByText("CN=测试上游 CA")).toBeVisible();
    expect(screen.getByText("AA:BB:CC:DD")).toBeVisible();
  });

  it("在当前监听内展示 Rust 解析的证书主题、SAN、有效期和指纹", async () => {
    const serverIdentity = certificateReference("server-ref", "本入口服务端身份", "reverse_server_identity");
    const clientTrust = certificateReference("client-ca-ref", "客户端证书 CA", "downstream_client_trust");
    const listener = {
      ...fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
      downstream_tls: {
        enabled: true,
        server_identity: serverIdentity.id,
        client_authentication: { mode: "required" as const, trust: clientTrust.id },
      },
    };
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [listener],
      certificate_references: [serverIdentity, clientTrust],
    }));
    mocks.listenerCertificateOverview.mockReturnValue(ok([
      certificateDetail(serverIdentity, "CN=proxy.lan"),
      certificateDetail(clientTrust, "CN=Terminal Root CA"),
    ]));

    render(<ListenersView />);

    expect(await screen.findByText("CN=proxy.lan")).toBeVisible();
    expect(screen.getByText("CN=Terminal Root CA")).toBeVisible();
    expect(screen.getAllByText("DNS:server.test、IP:10.0.0.8")).toHaveLength(2);
    expect(screen.getAllByText("AA:BB:CC:DD")).toHaveLength(2);
  });

  it("导入 mTLS 身份时密码不进入 Workspace", async () => {
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")] }));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入 client.p12" }));
    expect(screen.getByText(/包含“客户端证书 \+ 私钥”的 client\.p12/)).toBeVisible();
    await user.type(await screen.findByLabelText("client.p12 / client.pfx 密码（允许为空）"), "p12-secret");
    await user.click(screen.getByRole("button", { name: "选择 client.p12 / .pfx" }));
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    const saved = mocks.workspaceSave.mock.calls[0][0];
    expect(saved.listeners[0].fixed_server.upstream_tls.client_identity).toBe("identity-ref-1");
    expect(JSON.stringify(saved)).not.toContain("p12-secret");
  });

  it("多个监听的固定 Server 与证书配置互不覆盖", async () => {
    const multiple = { ...workspace, listeners: [
      fixedListener("transaction", "Transaction", 16627, "https://transaction.test:16627"),
      fixedListener("dll", "DLL", 16127, "https://dll.test:16127"),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    const user = userEvent.setup(); render(<ListenersView />);
    const firstUrl = await screen.findByRole("textbox", { name: "固定 Server URL" });
    await user.clear(firstUrl); await user.type(firstUrl, "https://transaction-v2.test:16627");
    await user.click(screen.getByText("DLL"));
    expect(await screen.findByRole("textbox", { name: "固定 Server URL" })).toHaveValue("https://dll.test:16127");
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    const saved = mocks.workspaceSave.mock.calls[0][0];
    expect(saved.listeners[0].fixed_server.upstream_url).toBe("https://transaction-v2.test:16627");
    expect(saved.listeners[1].fixed_server.upstream_url).toBe("https://dll.test:16127");
  });

  it("直接为当前监听选择请求和响应正文编码", async () => {
    const fixedWorkspace = {
      ...workspace,
      listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup(); render(<ListenersView />);

    expect(await screen.findByText("HTTP 正文编码")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /请求正文编码/ }));
    await user.click(await screen.findByRole("option", { name: "Shift-JIS" }));
    await user.click(screen.getByRole("button", { name: /响应正文编码/ }));
    await user.click(await screen.findByRole("option", { name: "UTF-8" }));
    await user.click(screen.getByRole("button", { name: "校验并保存" }));

    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    const savedListener = mocks.workspaceSave.mock.calls[0][0].listeners[0];
    expect(savedListener.request_body_codec).toBe("shift_jis");
    expect(savedListener.response_body_codec).toBe("utf8");
  });

  it("动态目标监听的 Basic 密码只进入 Rust 安全存储", async () => {
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("switch", { name: "启用 HTTP Basic 认证" }));
    await user.type(screen.getByRole("textbox", { name: "代理认证用户名" }), "operator");
    await user.type(screen.getByLabelText("代理认证密码"), "secret");
    await user.click(screen.getByRole("button", { name: "保护并引用" }));
    expect(mocks.workspaceSecretStoreBasic).toHaveBeenCalledWith("operator", "secret");
    expect(await screen.findByText(/system\/secret-ref-1/)).toBeVisible();
  });

  it("说明监听流量如何进入故障模拟", async () => {
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "去添加故障模拟" }));
    expect(navigationMocks.navigate).toHaveBeenCalledWith("/faults");
  });
});
