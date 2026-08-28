import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProxyListener, ProxyWorkspace } from "@/generated/rust-types";
import {
  bootstrap, certificateReference, commandError, listenerOverview, listenerStatus,
  localResponderListener, mocks, navigationMocks, ok, setupListenerMocks,
  socketListener, workspace,
} from "./listeners-view.test-support";

const uiMocks = vi.hoisted(() => ({ toast: vi.fn() }));
vi.mock("@heroui/react", async (importOriginal) => ({
  ...await importOriginal<typeof import("@heroui/react")>(), toast: uiMocks.toast,
}));
vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => navigationMocks,
  useWorkspaceQueryInvalidation: vi.fn(),
}));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined, useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

function scriptedSocketWorkspace(): ProxyWorkspace {
  const listener: ProxyListener = socketListener("socket-1", "Socket TLS", 9443, "tls_to_tls");
  if (listener.data_plane.kind !== "socket") throw new Error("expected Socket fixture");
  listener.data_plane.settings.processing = {
    mode: "scripted",
    settings: {
      package: { id: "iso-8583", version: "1.0.0" },
    },
  };
  const references = [
    certificateReference("app-id", "App Identity", "reverse_server_identity"),
    certificateReference("server-ca", "Server CA", "upstream_server_trust"),
  ];
  const topology = listener.data_plane.settings.topology;
  if (topology.mode !== "relay" || topology.settings.security.mode !== "tls_to_tls") throw new Error("expected TLS Relay fixture");
  topology.settings.security.downstream_tls.server_identity = "app-id";
  topology.settings.security.upstream_tls.server_trust = "server-ca";
  return {
    ...workspace,
    listeners: [listener],
    certificate_references: references,
    android_network_profiles: [],
  };
}

function setupScriptedSocket() {
  const current = scriptedSocketWorkspace();
  mocks.workspaceGet.mockReturnValue(ok(current));
  mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus("socket-1")])));
  mocks.listenerProtocolPackageCatalog.mockReturnValue(ok({
    options: [{
      package: { id: "iso-8583", version: "1.0.0" }, name: "ISO 8583",
      kind: "socket",
      capabilities: {
        upstream: { frame: true, decode: true, encode: true },
        downstream: { frame: true, decode: true, encode: true }, display: true,
      },
      upstream_schema: { id: "iso-request", version: 1, title: "ISO Request", fields: [{ name: "mti", label: "MTI", type: "string" }] },
      downstream_schema: { id: "iso-response", version: 1, title: "ISO Response", fields: [{ name: "response_code", label: "Response", type: "string" }] },
    }],
    installed_version_count: 1, unavailable_version_count: 0, recommended_package: null,
  }));
  return current;
}

function expectSocketEditorLocked() {
  expect(screen.getByRole("textbox", { name: "代理监听名称" })).toBeDisabled();
  expect(screen.getByLabelText("Socket 响应方式")).toBeDisabled();
  expect(screen.getByLabelText("Socket 协议处理方案")).toBeDisabled();
  expect(screen.getByLabelText("App 接入传输")).toBeDisabled();
  expect(screen.getByLabelText("App 侧服务端身份")).toBeDisabled();
  expect(screen.getByLabelText("Server 传输")).toBeDisabled();
  expect(screen.getByLabelText("Server CA")).toBeDisabled();
  for (const control of screen.getAllByRole("switch")) expect(control).toBeDisabled();
}

describe("Listener Socket integration contracts", () => {
  beforeEach(setupListenerMocks);

  it("shows rejected save field paths and messages in a persistent Alert", async () => {
    setupScriptedSocket();
    const field = "listeners[0].data_plane.settings.processing.package";
    mocks.listenerSave.mockReturnValue(commandError("协议包版本已不可用。", {
      [field]: ["当前精确协议包版本未启用或已失效。"],
    }));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(uiMocks.toast).toHaveBeenCalledWith(
      "操作未完成，请按页面提示修正 Socket 配置。", { variant: "danger" },
    ));
    expect(await screen.findAllByText((text) => text.includes(field)
      && text.includes("当前精确协议包版本未启用或已失效"))).toHaveLength(2);
    expect(screen.getByText("精确协议包需要修正")).toBeVisible();
  });

  it("shows rejected start field paths and messages in a persistent Alert", async () => {
    setupScriptedSocket();
    const field = "listeners[0].data_plane.settings.processing";
    mocks.listenerStart.mockReturnValue(commandError("协议处理链不可用。", {
      [field]: ["当前包无法执行完整处理链。"],
    }));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "启动监听" }));
    await waitFor(() => expect(uiMocks.toast).toHaveBeenCalledWith(
      "操作未完成，请按页面提示修正 Socket 配置。", { variant: "danger" },
    ));
    expect(await screen.findAllByText((text) => text.includes(field)
      && text.includes("当前包无法执行完整处理链"))).toHaveLength(2);
    expect(screen.getByText("协议处理需要修正")).toBeVisible();
  });

  it("rejects a malformed catalog without rendering candidates or causing mutations", async () => {
    const local = localResponderListener();
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [local] }));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus(local.id)])));
    mocks.listenerProtocolPackageCatalog.mockReturnValue(ok({
      options: [{ package: { id: "bad", version: "1.0.0" } }], installed_version_count: 1, unavailable_version_count: 0, recommended_package: null,
    } as never));
    render(<ListenersView />);
    expect(await screen.findByText("协议包目录读取失败")).toBeVisible();
    expect(screen.getByText("入口协议包目录数据不完整，请刷新后重试。")).toBeVisible();
    expect(screen.queryByRole("option", { name: /bad@1\.0\.0/ })).not.toBeInTheDocument();
    expect(mocks.listenerSave).not.toHaveBeenCalled();
  });

  it("locks the entire editor while save is pending and preserves the snapshot after resolve", async () => {
    const current = setupScriptedSocket();
    const request = deferred<unknown>();
    mocks.listenerSave.mockReturnValue(request.promise as never);
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "保存当前监听" }));
    expect(await screen.findByRole("button", { name: "保存中…" })).toBeDisabled();
    expectSocketEditorLocked();
    fireEvent.change(screen.getByRole("textbox", { name: "代理监听名称" }), { target: { value: "并发污染" } });
    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue("Socket TLS");
    expect(mocks.listenerImportUpstreamServerTrust).not.toHaveBeenCalled();
    request.resolve({ status: "ok", data: { ...current, revision: 2 } });
    await waitFor(() => expect(screen.getByRole("button", { name: "保存当前监听" })).toBeEnabled());
    expect(screen.getByRole("textbox", { name: "代理监听名称" })).toHaveValue("Socket TLS");
  });

  it("locks the entire Listener editor while start is pending", async () => {
    setupScriptedSocket();
    const request = deferred<unknown>();
    mocks.listenerStart.mockReturnValue(request.promise as never);
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "启动监听" }));
    expect(await screen.findByRole("button", { name: "启动中…" })).toBeDisabled();
    expectSocketEditorLocked();
    expect(mocks.listenerSave).not.toHaveBeenCalled();
  });

  it("locks the entire Listener editor while validation is pending", async () => {
    setupScriptedSocket();
    const request = deferred<unknown>();
    mocks.listenerValidate.mockReturnValue(request.promise as never);
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "校验当前监听" }));
    expect(await screen.findByRole("button", { name: "校验中…" })).toBeDisabled();
    expectSocketEditorLocked();
    expect(mocks.listenerSave).not.toHaveBeenCalled();
  });
});
