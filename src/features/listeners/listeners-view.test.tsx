// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrap, setupListenerMocks, mocks, workspace, dynamicListener, fixedListener, socketListener, localResponderListener, ok, commandError, listenerStatus, listenerOverview, navigationMocks } from "./listeners-view.test-support";

vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("统一代理监听编辑器", () => {
  beforeEach(setupListenerMocks);

  it("只提供一个新建入口并调用无参数 Rust command", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);
    expect(await screen.findByRole("button", { name: "新建代理监听" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /新增正向|新增转发/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "新建代理监听" }));
    expect(mocks.listenerNew).toHaveBeenCalledWith();
    expect(await screen.findByRole("textbox", { name: "代理监听名称" })).toHaveValue("新建代理监听");
  });

  it("未保存的新建或复制监听无需 Rust runtime capability 即可本地丢弃", async () => {
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "新建代理监听" }));
    const deleteButton = screen.getByRole("button", { name: "删除监听" });
    expect(deleteButton).toBeEnabled();
    await user.click(deleteButton);

    expect(await screen.findByRole("textbox", { name: "代理监听名称" })).toHaveValue("默认代理监听");
    await user.click(screen.getByRole("button", { name: "复制监听" }));
    expect(await screen.findByRole("textbox", { name: "代理监听名称" })).toHaveValue("默认代理监听 副本");
    await user.click(screen.getByRole("button", { name: "删除监听" }));

    expect(await screen.findByRole("textbox", { name: "代理监听名称" })).toHaveValue("默认代理监听");
    expect(mocks.listenerDelete).not.toHaveBeenCalled();
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
    const serverUrl = await screen.findByRole("textbox", { name: "固定 Server URL" });
    await user.type(serverUrl, "https://server.test:443");
    expect(screen.getByText("固定 Server 目标")).toBeVisible();
    expect(screen.getByText(/仅用 Server URL 替换目标 host\/port/)).toBeVisible();
    expect(screen.getByText(/原请求 path 与 query 原样保留/)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "允许的客户端 CIDR" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "启用 HTTP Basic 认证" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "为此监听启用 TLS" })).toBeVisible();
    expect(screen.queryByRole("switch", { name: "启用 allowlist MITM" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "校验上游服务器主机名" })).toBeChecked();
    expect(screen.getByRole("button", { name: "导入 Server CA" })).toBeVisible();
    expect(screen.getByRole("button", { name: "导入客户端身份" })).toBeVisible();
    expect(screen.queryByRole("textbox", { name: /Body Codec 引用/ })).not.toBeInTheDocument();
    expect(screen.getByText(/自动模式遵循 Header；强制模式覆盖 charset/)).toBeVisible();
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
    await user.type(
      screen.getByRole("textbox", { name: "固定 Server URL" }),
      "https://server.test:443",
    );
    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
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
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
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

    expect(await screen.findByText("运行状态：未知（当前监听状态不可用）")).toBeVisible();
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

  it("Rust 未授予启停 capability 时不推断启动但仍允许删除已确认 stopped 的配置", async () => {
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-1", "stopped", { canStart: false, canStop: false }),
    ])));
    render(<ListenersView />);

    expect(await screen.findByRole("button", { name: "无可用操作" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除监听" })).toBeEnabled();
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

  it("保存 Socket 时只提交 active variant，不泄漏隐藏的 HTTP 或 TLS 字段", async () => {
    const socket = socketListener("socket-1", "Socket", 9000, "transparent");
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("socket-1")])));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledOnce());
    const saved = mocks.listenerSave.mock.calls[0][2];
    expect(saved.data_plane).toEqual(socket.data_plane);
    expect(JSON.stringify(saved.data_plane)).not.toMatch(
      /authentication|mitm|fixed_server|downstream_tls|request_body_codec/,
    );
  });

  it("Socket 连接探测 pending 时禁止重复触发", async () => {
    const socket = socketListener("socket-1", "Socket", 9000, "tcp_to_tls");
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("socket-1")])));
    mocks.listenerTestUpstreamConnection.mockReturnValue(new Promise(() => undefined));
    const user = userEvent.setup();
    render(<ListenersView />);

    const testButton = await screen.findByRole("button", { name: "测试 Server 连接" });
    await user.click(testButton);

    expect(await screen.findByRole("button", { name: "正在探测 Server…" })).toBeDisabled();
    expect(mocks.listenerValidate).toHaveBeenCalledOnce();
    expect(mocks.listenerTestUpstreamConnection).toHaveBeenCalledOnce();
  });

  it("LocalResponder 可启动和复制且不读取或探测不存在的 Server 上游", async () => {
    const local = localResponderListener();
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [local] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus(local.id)])));
    const user = userEvent.setup();
    render(<ListenersView />);

    expect(await screen.findByText("→ 本地应答")).toBeVisible();
    expect(screen.getByText("Socket · 本地应答")).toBeVisible();
    expect(screen.getByRole("button", { name: "启动监听" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "复制监听" })).toBeEnabled();
    expect(screen.queryByRole("textbox", { name: "Socket Server 主机" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "测试 Server 连接" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "复制监听" }));
    expect(mocks.listenerCopy).toHaveBeenCalledWith(local);
    expect(mocks.listenerTestUpstreamConnection).not.toHaveBeenCalled();
  });

  it("Socket 运行卡展示 Rust 协议标签、活动连接和双向字节", async () => {
    const socket = socketListener("socket-1", "Socket 入口", 9000, "tls_to_tls");
    const running = {
      ...listenerStatus("socket-1", "running"),
      kind_text: "Socket · TLS → TLS",
    };
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([running])));

    render(<ListenersView />);

    expect(await screen.findByText("Socket · TLS → TLS")).toBeVisible();
    expect(await screen.findByText(
      "活动连接 2 · C→S 1.0 KiB · S→C 2.0 KiB",
    )).toBeVisible();
  });

  it("Socket 启动和停止请求 pending 时显示精确状态并禁止重复操作", async () => {
    const socket = socketListener("socket-1", "Socket 入口", 9000, "transparent");
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("socket-1")])))
    mocks.listenerStart.mockReturnValue(new Promise(() => undefined));
    const user = userEvent.setup();
    const view = render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "启动监听" }));
    expect(await screen.findByRole("button", { name: "启动中…" })).toBeDisabled();
    expect(mocks.listenerStart).toHaveBeenCalledOnce();

    view.unmount();
    setupListenerMocks();
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [socket] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("socket-1", "running"),
    ])));
    mocks.listenerStop.mockReturnValue(new Promise(() => undefined));
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "停止监听" }));
    expect(await screen.findByRole("button", { name: "停止中…" })).toBeDisabled();
    expect(mocks.listenerStop).toHaveBeenCalledOnce();
  });

});
