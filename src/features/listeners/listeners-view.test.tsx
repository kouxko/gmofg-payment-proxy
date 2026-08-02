// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ListenersView } from "./listeners-view";

const navigationMocks = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => navigationMocks,
}));

const mocks = vi.hoisted(() => ({
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
  workspaceValidate: vi.fn(),
  workspaceSave: vi.fn(),
  listenerNew: vi.fn(),
  listenerCopy: vi.fn(),
  listenerStatuses: vi.fn(),
  listenerStart: vi.fn(),
  listenerStop: vi.fn(),
  listenerTestUpstreamTls: vi.fn(),
  workspaceSecretStoreBasic: vi.fn(),
}));

const workspace = {
  id: "workspace-1", name: "API Lab", revision: 1,
  listeners: [{
    kind: "forward" as const, id: "listener-1", name: "默认正向代理", enabled: false,
    bind_address: "127.0.0.1", port: 8080, authentication: { mode: "none" as const },
    allowed_client_cidrs: [], mitm: { enabled: false, authority_allowlist: [], root_ca: null, maximum_cached_leaf_certificates: 256 },
    connect_timeout_ms: 30000, read_timeout_ms: 70000, write_timeout_ms: 70000,
  }],
  body_codec_policies: [], metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [],
};

function reverseListener(id: string, name: string, port: number, upstreamUrl: string) {
  return {
    kind: "reverse" as const,
    id,
    name,
    enabled: false,
    bind_address: "127.0.0.1",
    port,
    upstream_url: upstreamUrl,
    downstream_tls: { enabled: false, server_identity: null, client_authentication: { mode: "disabled" as const } },
    upstream_tls: { verify_hostname: true, server_trust: null, client_identity: null },
    request_codec_policy: null,
    response_codec_policy: null,
  };
}

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

function ok<T>(data: T) { return Promise.resolve({ status: "ok" as const, data }); }

describe("Listeners workspace editor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.workspaceList.mockReturnValue(ok([{ id: "workspace-1", name: "API Lab", revision: 1, listener_count: 1, enabled_listener_count: 0, selected: true }]));
    mocks.workspaceGet.mockReturnValue(ok(workspace));
    mocks.workspaceValidate.mockImplementation((draft) => ok({ valid: true, normalized: draft, field_errors: {} }));
    mocks.workspaceSave.mockImplementation((draft) => ok({ ...draft, revision: 2 }));
    mocks.listenerNew.mockImplementation((kind) => ok(kind === "reverse"
      ? reverseListener("listener-new", "固定上游入口", 8443, "")
      : workspace.listeners[0]));
    mocks.listenerCopy.mockImplementation((source) => ok({
      ...source,
      id: "listener-copy",
      name: `${source.name} 副本`,
      enabled: false,
    }));
    mocks.listenerStatuses.mockReturnValue(ok([]));
    mocks.listenerTestUpstreamTls.mockReturnValue(ok({
      listener_id: "reverse-1",
      upstream_origin: "https://127.0.0.1:9443",
      resolved_address: "127.0.0.1:9443",
      tls_version: "TLS 1.2",
      cipher_suite: "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
      peer_subject: "CN=测试上游",
      peer_sha256_fingerprint: "AA:BB:CC",
      hostname_verification_enabled: true,
      client_identity_configured: true,
      elapsed_millis: 12,
      message: "上游 Server TLS 握手成功。",
      ui_tone: "positive",
    }));
    mocks.workspaceSecretStoreBasic.mockReturnValue(ok({ provider: "system", key: "secret-ref-1" }));
  });

  it("explains how entry configuration connects to fault simulation", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "去添加故障模拟" }));
    expect(navigationMocks.navigate).toHaveBeenCalledWith("/faults");
  });

  it("validates in Rust before saving the edited listener", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理入口名称" });
    await user.clear(name);
    await user.type(name, "Local Forward");
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    await waitFor(() => expect(mocks.workspaceValidate).toHaveBeenCalledTimes(1));
    expect(mocks.workspaceSave).toHaveBeenCalledTimes(1);
    expect(mocks.workspaceSave.mock.calls[0][0].listeners[0].name).toBe("Local Forward");
  });

  it("does not save when Rust rejects the workspace", async () => {
    mocks.workspaceValidate.mockImplementation((draft) => ok({ valid: false, normalized: draft, field_errors: { "listeners.0.port": ["端口无效"] } }));
    const user = userEvent.setup();
    render(<ListenersView />);
    await screen.findByRole("textbox", { name: "代理入口名称" });
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    expect(await screen.findByText(/端口无效/)).toBeVisible();
    expect(mocks.workspaceSave).not.toHaveBeenCalled();
  });

  it("persists the selected reverse listener and performs a real upstream TLS test in Rust", async () => {
    const reverseWorkspace = {
      ...workspace,
      listeners: [reverseListener("reverse-1", "交易入口", 16627, "https://127.0.0.1:9443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(reverseWorkspace));
    mocks.workspaceSave.mockImplementation((draft) => ok({ ...draft, revision: 2 }));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "测试上游 TLS 握手" }));

    await waitFor(() => expect(mocks.workspaceValidate).toHaveBeenCalledWith(reverseWorkspace));
    expect(mocks.workspaceSave).toHaveBeenCalledTimes(1);
    expect(mocks.listenerTestUpstreamTls).toHaveBeenCalledWith("workspace-1", "reverse-1");
    expect(await screen.findByText(/127.0.0.1:9443 · 12 ms/)).toBeVisible();
    expect(screen.getByText(/TLS 1.2/)).toBeVisible();
    expect(screen.getByText(/客户端身份：已配置/)).toBeVisible();
  });

  it("clears a successful TLS result as soon as reverse TLS inputs change", async () => {
    const reverseWorkspace = {
      ...workspace,
      listeners: [reverseListener("reverse-1", "交易入口", 16627, "https://127.0.0.1:9443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(reverseWorkspace));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "测试上游 TLS 握手" }));
    expect(await screen.findByText("上游 Server TLS 握手成功。")).toBeVisible();

    await user.type(screen.getByRole("textbox", { name: "上游 CA 引用" }), "ca-ref");
    expect(screen.queryByText("上游 Server TLS 握手成功。")).not.toBeInTheDocument();
  });

  it("clears a failed TLS result as soon as reverse TLS inputs change", async () => {
    const reverseWorkspace = {
      ...workspace,
      listeners: [reverseListener("reverse-1", "交易入口", 16627, "https://127.0.0.1:9443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(reverseWorkspace));
    mocks.listenerTestUpstreamTls.mockReturnValue(Promise.resolve({
      status: "error" as const,
      error: { code: "TLS_FAILED", message: "证书不受信任", field_errors: {}, retryable: false },
    }));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "测试上游 TLS 握手" }));
    expect(await screen.findByText("证书不受信任")).toBeVisible();

    await user.clear(screen.getByRole("textbox", { name: "上游 URL" }));
    expect(screen.queryByText("证书不受信任")).not.toBeInTheDocument();
  });

  it("selects optional or required downstream client authentication without resetting the field", async () => {
    const listener = reverseListener("reverse-1", "交易入口", 16627, "https://127.0.0.1:9443");
    listener.downstream_tls.enabled = true;
    const reverseWorkspace = { ...workspace, listeners: [listener] };
    mocks.workspaceGet.mockReturnValue(ok(reverseWorkspace));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByLabelText("下游客户端认证模式"));
    await user.click(await screen.findByRole("option", { name: "必须提供客户端证书" }));
    expect(await screen.findByRole("textbox", { name: "下游客户端 CA 引用" })).toBeVisible();
    await user.type(screen.getByRole("textbox", { name: "下游客户端 CA 引用" }), "client-ca-ref");
    await user.click(screen.getByRole("button", { name: "校验并保存" }));

    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    expect(mocks.workspaceSave.mock.calls[0][0].listeners[0].downstream_tls.client_authentication)
      .toEqual({ mode: "required", trust: "client-ca-ref" });
  });

  it("stores Basic credentials in Rust and keeps only the protected reference in Workspace", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    await screen.findByRole("textbox", { name: "代理入口名称" });
    await user.click(screen.getByRole("switch", { name: "启用 HTTP Basic 认证" }));
    await user.type(screen.getByRole("textbox", { name: "代理认证用户名" }), "operator");
    await user.type(screen.getByLabelText("代理认证密码"), "secret");
    await user.click(screen.getByRole("button", { name: "保护并引用" }));

    await waitFor(() => expect(mocks.workspaceSecretStoreBasic).toHaveBeenCalledWith("operator", "secret"));
    expect(await screen.findByText(/system\/secret-ref-1/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    const saved = mocks.workspaceSave.mock.calls[0][0];
    expect(saved.listeners[0].authentication).toEqual({
      mode: "basic",
      credential: { provider: "system", key: "secret-ref-1" },
    });
    expect(JSON.stringify(saved)).not.toContain("operator");
    expect(JSON.stringify(saved)).not.toContain("secret\"");
  });

  it("keeps different reverse ports and upstream origins as independent mappings", async () => {
    const multiMappingWorkspace = {
      ...workspace,
      listeners: [
        reverseListener("listener-transaction", "Transaction", 16627, "https://transaction.example.test:16627"),
        reverseListener("listener-dll", "DLL", 16127, "https://dll.example.test:16127"),
      ],
    };
    mocks.workspaceGet.mockReturnValue(ok(multiMappingWorkspace));
    const user = userEvent.setup();
    render(<ListenersView />);

    const transactionUpstream = await screen.findByRole("textbox", { name: "上游 URL" });
    await user.clear(transactionUpstream);
    await user.type(transactionUpstream, "https://transaction-v2.example.test:16627");
    await user.click(screen.getByText("DLL"));
    const dllUpstream = await screen.findByRole("textbox", { name: "上游 URL" });
    expect(dllUpstream).toHaveValue("https://dll.example.test:16127");
    await user.clear(dllUpstream);
    await user.type(dllUpstream, "https://dll-v2.example.test:16128");

    await user.click(screen.getByRole("button", { name: "校验并保存" }));
    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledTimes(1));
    const saved = mocks.workspaceSave.mock.calls[0][0];
    expect(saved.listeners).toEqual(expect.arrayContaining([
      expect.objectContaining({
        id: "listener-transaction",
        port: 16627,
        upstream_url: "https://transaction-v2.example.test:16627",
      }),
      expect.objectContaining({
        id: "listener-dll",
        port: 16127,
        upstream_url: "https://dll-v2.example.test:16128",
      }),
    ]));
  });

  it("asks Rust to copy a listener before adding a new independent mapping", async () => {
    const multiMappingWorkspace = {
      ...workspace,
      listeners: [reverseListener("listener-transaction", "Transaction", 16627, "https://transaction.example.test:16627")],
    };
    mocks.workspaceGet.mockReturnValue(ok(multiMappingWorkspace));
    const user = userEvent.setup();
    render(<ListenersView />);

    await screen.findByRole("textbox", { name: "上游 URL" });
    await user.click(screen.getByRole("button", { name: "复制为新入口" }));

    await waitFor(() => expect(mocks.listenerCopy).toHaveBeenCalledWith(multiMappingWorkspace.listeners[0]));
    expect(await screen.findByRole("textbox", { name: "代理入口名称" })).toHaveValue("Transaction 副本");
  });
});
