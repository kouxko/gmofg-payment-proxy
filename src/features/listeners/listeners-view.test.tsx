// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ListenersView } from "./listeners-view";

const navigationMocks = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({
    bootstrap: {
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
    },
  }),
}));

const mocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), workspaceValidate: vi.fn(), workspaceSave: vi.fn(),
  listenerValidate: vi.fn(),
  listenerNew: vi.fn(), listenerCopy: vi.fn(), listenerSave: vi.fn(), listenerDelete: vi.fn(),
  listenerOverview: vi.fn(), listenerStart: vi.fn(), listenerStop: vi.fn(),
  listenerTestUpstreamTls: vi.fn(), listenerImportUpstreamClientIdentity: vi.fn(), listenerImportUpstreamServerTrust: vi.fn(),
  listenerImportDownstreamServerIdentity: vi.fn(), listenerImportDownstreamClientTrust: vi.fn(),
  listenerCertificateOverview: vi.fn(), listenerCertificateDiscard: vi.fn(),
  workspaceSecretStoreBasic: vi.fn(),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

function dynamicListener(id = "listener-1", name = "默认代理监听", port = 8080) {
  return {
    id, name, enabled: false, bind_address: "127.0.0.1", port,
    authentication: { mode: "none" as const }, allowed_client_cidrs: [],
    mitm: { enabled: false, authority_allowlist: [], root_ca: null, maximum_cached_leaf_certificates: 256 },
    connect_timeout_ms: 30000, read_timeout_ms: 70000, write_timeout_ms: 70000,
    downstream_tls: {
      enabled: false,
      server_identity: null,
      dynamic_sni_allowlist: [],
      client_authentication: { mode: "disabled" as const },
    },
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

function commandError(message: string) {
  return Promise.resolve({
    status: "error" as const,
    error: {
      code: "LISTENER_OVERVIEW_FAILED",
      message,
      field_errors: {},
      retryable: true,
      suggested_action: "请重试。",
    },
  });
}

function listenerStatus(
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
  };
}

function listenerOverview(rows = [listenerStatus("listener-1")]) {
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

describe("统一代理监听编辑器", () => {
  beforeEach(() => {
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
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview()));
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
    const downstreamIdentity = certificateReference("downstream-identity-ref-1", "入口服务端身份", "reverse_server_identity");
    const downstreamTrust = certificateReference("downstream-ca-ref-1", "终端客户端 CA", "downstream_client_trust");
    mocks.listenerImportUpstreamClientIdentity.mockReturnValue(ok({ reference: identity, detail: certificateDetail(identity, "CN=测试客户端身份") }));
    mocks.listenerImportUpstreamServerTrust.mockReturnValue(ok({ reference: trust, detail: certificateDetail(trust, "CN=测试上游 CA") }));
    mocks.listenerImportDownstreamServerIdentity.mockReturnValue(ok({ reference: downstreamIdentity, detail: certificateDetail(downstreamIdentity, "CN=proxy.test") }));
    mocks.listenerImportDownstreamClientTrust.mockReturnValue(ok({ reference: downstreamTrust, detail: certificateDetail(downstreamTrust, "CN=终端客户端 CA") }));
    mocks.listenerCertificateOverview.mockReturnValue(ok([]));
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
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].name).toBe("本地代理");
  });

  it("配置未修改时可在其他监听运行中直接启动第二个监听", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待启动监听", 8081),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("running-1", "running"), listenerStatus("stopped-2")])));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待启动监听"));
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 1, "stopped-2"));
    expect(mocks.listenerValidate).not.toHaveBeenCalled();
    expect(mocks.listenerSave).not.toHaveBeenCalled();
  });

  it("其他监听运行时仍保存当前脏草稿并启动", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待启动监听", 8081),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("running-1", "running"), listenerStatus("stopped-2")])));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待启动监听"));
    const name = screen.getByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "修改后的监听");
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].name).toBe("修改后的监听");
    expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 2, "stopped-2");
  });

  it("启动 B 时保留 A 的未保存草稿", async () => {
    const listenerA = dynamicListener("listener-a", "监听 A", 8080);
    const listenerB = dynamicListener("listener-b", "监听 B", 8081);
    const multiple = { ...workspace, listeners: [listenerA, listenerB] };
    const afterSave = { ...multiple, revision: 2 };
    const afterStart = {
      ...multiple,
      revision: 3,
      listeners: [listenerA, { ...listenerB, enabled: true }],
    };
    mocks.workspaceGet
      .mockReturnValueOnce(ok(multiple))
      .mockReturnValue(ok(afterStart));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-a"),
      listenerStatus("listener-b"),
    ])));
    mocks.listenerValidate.mockImplementation((_workspaceId, revision, listener, certificateReferences) => ok({
      valid: true,
      normalized: {
        ...multiple,
        revision,
        listeners: multiple.listeners.map((item) => item.id === listener.id ? listener : item),
        certificate_references: certificateReferences,
      },
      field_errors: {},
    }));
    mocks.listenerSave.mockReturnValue(ok(afterSave));
    const user = userEvent.setup();
    render(<ListenersView />);

    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "监听 A 未保存名称");
    await user.click(screen.getByText("监听 B"));
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerStart).toHaveBeenCalledWith(
      "workspace-1",
      2,
      "listener-b",
    ));
    await user.click(screen.getByText("监听 A 未保存名称"));
    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue("监听 A 未保存名称");
  });

  it("保存 B 时保留新建 A 及其未保存托管证书引用", async () => {
    const listenerB = dynamicListener("listener-b", "监听 B", 8080);
    const persisted = { ...workspace, listeners: [listenerB] };
    mocks.workspaceGet.mockReturnValue(ok(persisted));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("listener-b")] )));
    mocks.listenerValidate.mockImplementation((_workspaceId, revision, listener, certificateReferences) => ok({
      valid: true,
      normalized: {
        ...persisted,
        revision,
        listeners: persisted.listeners.map((item) => item.id === listener.id ? listener : item),
        certificate_references: certificateReferences,
      },
      field_errors: {},
    }));
    mocks.listenerSave.mockImplementation((_workspaceId, _revision, listener, certificateReferences) => ok({
      ...persisted,
      revision: 2,
      listeners: [listener],
      certificate_references: certificateReferences,
    }));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "新建代理监听" }));
    await user.click(screen.getByRole("switch", { name: "转发到固定 Server" }));
    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.crt / .pem）" }));
    await user.click(screen.getByText("监听 B"));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("row", { name: /新建代理监听/ }));
    expect(await screen.findByText("CN=测试上游 CA")).toBeVisible();
    expect(mocks.listenerCertificateDiscard).not.toHaveBeenCalled();
  });

  it("其他监听运行时仍可删除当前已停止监听", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待删除监听", 8081),
    ] };
    const afterDelete = { ...multiple, revision: 2, listeners: [multiple.listeners[0]] };
    mocks.workspaceGet
      .mockReturnValueOnce(ok(multiple))
      .mockReturnValue(ok(afterDelete));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("running-1", "running"),
      listenerStatus("stopped-2"),
    ])));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待删除监听"));
    await user.click(screen.getByRole("button", { name: "删除监听" }));

    await waitFor(() => expect(mocks.listenerDelete).toHaveBeenCalledWith(
      "workspace-1",
      1,
      "stopped-2",
    ));
    expect(mocks.workspaceSave).not.toHaveBeenCalled();
  });

  it("删除 B 时保留 A 的脏草稿和未保存托管证书引用", async () => {
    const listenerA = fixedListener("listener-a", "监听 A", 16627, "https://a.test:16627");
    const listenerB = dynamicListener("listener-b", "监听 B", 16127);
    const multiple = { ...workspace, listeners: [listenerA, listenerB] };
    const afterDelete = { ...multiple, revision: 2, listeners: [listenerA] };
    mocks.workspaceGet
      .mockReturnValueOnce(ok(multiple))
      .mockReturnValue(ok(afterDelete));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-a"),
      listenerStatus("listener-b"),
    ])));
    const user = userEvent.setup();
    render(<ListenersView />);

    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name);
    await user.type(name, "监听 A 未保存名称");
    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.crt / .pem）" }));
    await user.click(screen.getByText("监听 B"));
    await user.click(screen.getByRole("button", { name: "删除监听" }));

    expect(await screen.findByRole("textbox", { name: "代理监听名称" })).toHaveValue("监听 A 未保存名称");
    expect(screen.getByText("CN=测试上游 CA")).toBeVisible();
    expect(mocks.listenerCertificateDiscard).not.toHaveBeenCalled();
  });

  it("运行概览查询失败时显式报错且禁止启动", async () => {
    mocks.listenerOverview.mockReturnValue(commandError("无法读取 Listener 运行概览。"));
    render(<ListenersView />);

    expect(await screen.findByText("运行状态：查询失败")).toBeVisible();
    expect(screen.getByText("无法读取 Listener 运行概览。")).toBeVisible();
    expect(screen.getByRole("button", { name: "状态不可用" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "重试状态查询" })).toBeVisible();
  });

  it("Rust 概览缺少当前 Listener 行时显示未知且禁止启动", async () => {
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([])));
    render(<ListenersView />);

    expect(await screen.findByText("运行状态：未知（Rust 未返回当前监听状态）")).toBeVisible();
    expect(screen.getByRole("button", { name: "状态不可用" })).toBeDisabled();
    expect(mocks.listenerStart).not.toHaveBeenCalled();
  });

  it("故障 Listener 按 Rust capability 执行停止以释放 runtime ownership", async () => {
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-1", "faulted", { canStart: false, canStop: true }),
    ])));
    const user = userEvent.setup();
    render(<ListenersView />);

    expect(await screen.findByText("运行状态：故障")).toBeVisible();
    expect(screen.getByRole("button", { name: "删除监听" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "停止监听" }));

    await waitFor(() => expect(mocks.listenerStop).toHaveBeenCalledWith(
      "workspace-1",
      1,
      "listener-1",
    ));
    expect(mocks.listenerStart).not.toHaveBeenCalled();
  });

  it("Rust 未授予启停 capability 时不从 stopped 状态自行推断启动", async () => {
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-1", "stopped", { canStart: false, canStop: false }),
    ])));
    render(<ListenersView />);

    expect(await screen.findByRole("button", { name: "无可用操作" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除监听" })).toBeDisabled();
    expect(mocks.listenerStart).not.toHaveBeenCalled();
    expect(mocks.listenerStop).not.toHaveBeenCalled();
  });

  it("修改后恢复为持久化值时视为无未保存差异", async () => {
    const multiple = { ...workspace, listeners: [
      dynamicListener("running-1", "已运行监听", 8080),
      dynamicListener("stopped-2", "待启动监听", 8081),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("running-1", "running"), listenerStatus("stopped-2")])));
    const user = userEvent.setup(); render(<ListenersView />);

    await user.click(await screen.findByText("待启动监听"));
    const name = screen.getByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "临时名称");
    await user.clear(name); await user.type(name, "待启动监听");
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 1, "stopped-2"));
    expect(mocks.listenerSave).not.toHaveBeenCalled();
  });

  it("没有其他运行监听时仍先保存脏草稿再启动", async () => {
    const user = userEvent.setup(); render(<ListenersView />);
    const name = await screen.findByRole("textbox", { name: "代理监听名称" });
    await user.clear(name); await user.type(name, "修改后的监听");
    await user.click(screen.getByRole("button", { name: "启动监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerStart).toHaveBeenCalledWith("workspace-1", 2, "listener-1");
  });

  it("其他监听运行时仍可使用当前草稿测试固定 Server TLS", async () => {
    const fixedWorkspace = {
      ...workspace,
      listeners: [
        dynamicListener("running-1", "运行中的 DLL", 16127),
        fixedListener("fixed-1", "交易", 16627, "https://127.0.0.1:9443"),
      ],
    };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    mocks.listenerOverview.mockReturnValue(
      ok(listenerOverview([listenerStatus("running-1", "running"), listenerStatus("fixed-1")])),
    );
    mocks.listenerValidate.mockImplementation((_workspaceId, _revision, listener, certificateReferences) => ok({
      valid: true,
      normalized: {
        ...fixedWorkspace,
        listeners: fixedWorkspace.listeners.map((item) => item.id === listener.id ? listener : item),
        certificate_references: certificateReferences,
      },
      field_errors: {},
    }));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByText("交易"));
    await user.click(await screen.findByRole("button", { name: "测试上游 TLS / mTLS 握手" }));
    await waitFor(() => expect(mocks.listenerValidate).toHaveBeenCalledWith(
      fixedWorkspace.id,
      fixedWorkspace.revision,
      fixedWorkspace.listeners[1],
      fixedWorkspace.certificate_references,
    ));
    expect(mocks.listenerSave).not.toHaveBeenCalled();
    expect(mocks.listenerTestUpstreamTls).toHaveBeenCalledWith(
      fixedWorkspace.id,
      fixedWorkspace.revision,
      fixedWorkspace.listeners[1],
      [],
    );
    expect(await screen.findByText(/127.0.0.1:9443 · 12 ms/)).toBeVisible();
  });

  it("导入 CA 后只把安全引用绑定到当前固定 Server", async () => {
    const fixedWorkspace = { ...workspace, listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")] };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    expect(screen.getByText(/签发上游 Server 证书的 ca\.crt/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.crt / .pem）" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].fixed_server.upstream_tls.server_trust).toBe("ca-ref-1");
    expect(await screen.findByText("CN=测试上游 CA")).toBeVisible();
    expect(screen.getByText("AA:BB:CC:DD")).toBeVisible();
  });

  it("替换未保存的导入证书时清理 Rust 安全存储材料", async () => {
    const fixedWorkspace = {
      ...workspace,
      listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.crt / .pem）" }));
    await user.click(screen.getByRole("switch", { name: "转发到固定 Server" }));

    await waitFor(() => expect(mocks.listenerCertificateDiscard).toHaveBeenCalledWith(
      certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust"),
    ));
  });

  it("解除持久化证书绑定时不删除 Rust 安全存储材料", async () => {
    const trust = certificateReference("persisted-ca", "持久化 CA", "upstream_server_trust");
    const base = fixedListener("fixed-1", "交易", 16627, "https://server.test:443");
    const listener = {
      ...base,
      fixed_server: {
        ...base.fixed_server,
        upstream_tls: { ...base.fixed_server.upstream_tls, server_trust: trust.id },
      },
    };
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [listener],
      certificate_references: [trust],
    }));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("switch", { name: "转发到固定 Server" }));

    expect(mocks.listenerCertificateDiscard).not.toHaveBeenCalled();
  });

  it("在当前监听内展示 Rust 解析的证书主题、SAN、有效期和指纹", async () => {
    const serverIdentity = certificateReference("server-ref", "本入口服务端身份", "reverse_server_identity");
    const clientTrust = certificateReference("client-ca-ref", "客户端证书 CA", "downstream_client_trust");
    const listener = {
      ...fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
      downstream_tls: {
        enabled: true,
        server_identity: serverIdentity.id,
        dynamic_sni_allowlist: [],
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

  it("下游 TLS 留空时按允许的客户端 SNI 动态签发证书", async () => {
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [{
        ...fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        downstream_tls: {
          enabled: true,
          server_identity: null,
          dynamic_sni_allowlist: ["api.example.test"],
          client_authentication: { mode: "disabled" as const },
        },
      }],
    }));

    render(<ListenersView />);

    expect((await screen.findAllByText(/证书管理页 Root CA/))[0]).toBeVisible();
    expect(screen.getByText(/按允许的客户端 SNI 动态签发/)).toBeVisible();
    expect(screen.getByLabelText("动态 SNI 允许域名")).toHaveValue("api.example.test");
    expect(screen.getByText("CN=Intercept Proxy Root CA")).toBeVisible();
    expect(screen.getByText("55:66:77:88")).toBeVisible();
  });

  it("失效的外部服务端身份可一键恢复为证书页叶子证书", async () => {
    const staleIdentity = certificateReference("stale-server-ref", "已失效的服务端身份", "reverse_server_identity");
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [{
        ...fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        downstream_tls: {
          enabled: true,
          server_identity: staleIdentity.id,
          dynamic_sni_allowlist: [],
          client_authentication: { mode: "disabled" as const },
        },
      }],
      certificate_references: [staleIdentity],
    }));
    mocks.listenerCertificateOverview.mockReturnValue(ok([{
      reference_id: staleIdentity.id,
      label: staleIdentity.label,
      certificate: null,
      error_message: "无法读取导入文件：No such file or directory",
    }]));

    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "改用本机叶子证书" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].downstream_tls.server_identity).toBeNull();
  });

  it("导入独立下游身份后只保存受保护引用并显示解析详情", async () => {
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [{
        ...fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        downstream_tls: {
          enabled: true,
          server_identity: null,
          client_authentication: { mode: "disabled" as const },
        },
      }],
    }));

    const user = userEvent.setup();
    render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入独立服务端身份" }));
    await user.click(screen.getByRole("button", { name: "选择服务端身份 PEM" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].downstream_tls.server_identity).toBe("downstream-identity-ref-1");
    expect(mocks.listenerSave.mock.calls[0][3][0].reference).toBe("managed:downstream-identity-ref-1");
    expect(await screen.findByText("CN=proxy.test")).toBeVisible();
  });

  it("导入 mTLS 身份时密码不进入 Workspace", async () => {
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")] }));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入 client.p12" }));
    expect(screen.getByText(/包含“客户端证书 \+ 私钥”的 client\.p12/)).toBeVisible();
    await user.type(await screen.findByLabelText("client.p12 / client.pfx 密码（允许为空）"), "p12-secret");
    await user.click(screen.getByRole("button", { name: "选择 client.p12 / .pfx" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].fixed_server.upstream_tls.client_identity).toBe("identity-ref-1");
    expect(JSON.stringify(mocks.listenerSave.mock.calls[0])).not.toContain("p12-secret");
  }, 15_000);

  it("多个监听的固定 Server 与证书配置互不覆盖", async () => {
    const multiple = { ...workspace, listeners: [
      fixedListener("transaction", "Transaction", 16627, "https://transaction.test:16627"),
      fixedListener("dll", "DLL", 16127, "https://dll.test:16127"),
    ] };
    mocks.workspaceGet.mockReturnValue(ok(multiple));
    const user = userEvent.setup(); render(<ListenersView />);
    const firstUrl = await screen.findByRole("textbox", { name: "固定 Server URL" });
    fireEvent.change(firstUrl, { target: { value: "https://transaction-v2.test:16627" } });
    await user.click(screen.getByText("DLL"));
    expect(await screen.findByRole("textbox", { name: "固定 Server URL" })).toHaveValue("https://dll.test:16127");
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerSave.mock.calls[0][2].fixed_server.upstream_url).toBe("https://dll.test:16127");
    expect(mocks.listenerSave.mock.calls[0][2].id).toBe("dll");
  });

  it("保存监听时只提交该监听实际引用的证书材料", async () => {
    const downstreamIdentity = certificateReference("transaction-downstream", "交易入口服务端身份", "reverse_server_identity");
    const upstreamTrust = certificateReference("transaction-upstream-ca", "交易上游 CA", "upstream_server_trust");
    const otherIdentity = certificateReference("dll-downstream", "DLL 入口服务端身份", "reverse_server_identity");
    const transaction = {
      ...fixedListener("transaction", "Transaction", 16627, "https://transaction.test:16627"),
      downstream_tls: {
        enabled: true,
        server_identity: downstreamIdentity.id,
        client_authentication: { mode: "disabled" as const },
      },
      fixed_server: {
        upstream_url: "https://transaction.test:16627",
        upstream_tls: {
          verify_hostname: true,
          server_trust: upstreamTrust.id,
          client_identity: null,
        },
      },
    };
    const dll = {
      ...fixedListener("dll", "DLL", 16127, "https://dll.test:16127"),
      downstream_tls: {
        enabled: true,
        server_identity: otherIdentity.id,
        client_authentication: { mode: "disabled" as const },
      },
    };
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [transaction, dll],
      certificate_references: [downstreamIdentity, upstreamTrust, otherIdentity],
    }));

    const user = userEvent.setup();
    render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(mocks.listenerValidate.mock.calls[0][3].map((item: { id: string }) => item.id)).toEqual([
      downstreamIdentity.id,
      upstreamTrust.id,
    ]);
    expect(mocks.listenerSave.mock.calls[0][3].map((item: { id: string }) => item.id)).toEqual([
      downstreamIdentity.id,
      upstreamTrust.id,
    ]);
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
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    const savedListener = mocks.listenerSave.mock.calls[0][2];
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
