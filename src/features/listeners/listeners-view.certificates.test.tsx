// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrap, setupListenerMocks, mocks, workspace, dynamicListener, fixedListener, socketListener, certificateReference, ok, listenerStatus, listenerOverview, navigationMocks } from "./listeners-view.test-support";

vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("统一代理监听编辑器", () => {
  beforeEach(setupListenerMocks);

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
    await user.click(await screen.findByRole("button", { name: "测试 Server 连接" }));
    await waitFor(() => expect(mocks.listenerValidate).toHaveBeenCalledWith(
      fixedWorkspace.id,
      fixedWorkspace.revision,
      fixedWorkspace.listeners[1],
      fixedWorkspace.certificate_references,
    ));
    expect(mocks.listenerSave).not.toHaveBeenCalled();
    expect(mocks.listenerTestUpstreamConnection).toHaveBeenCalledWith(
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
    expect(screen.getByText(/签发上游 Server 证书的单个 CA 锚/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(
      mocks.listenerSave.mock.calls[0][2].data_plane.settings.fixed_server.upstream_tls.server_trust,
    ).toBe("ca-ref-1");
    expect(await screen.findByText("CN=测试上游 CA")).toBeVisible();
    expect(screen.getByText("AA:BB:CC:DD")).toBeVisible();
  });

  it("Socket Relay 导入上游 CA 后保留 topology 并只更新 Relay 安全配置", async () => {
    const socket = socketListener("socket-1", "Socket TLS", 9443, "tcp_to_tls");
    const socketWorkspace = { ...workspace, listeners: [socket] };
    mocks.workspaceGet.mockReturnValue(ok(socketWorkspace));
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([listenerStatus(socket.id)])));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    const saved = mocks.listenerSave.mock.calls[0][2];
    expect(saved.data_plane.settings.topology).toEqual({
      mode: "relay",
      settings: {
        upstream: { host: "server.test", port: 9443 },
        security: {
          mode: "tcp_to_tls",
          upstream_tls: {
            verify_hostname: true,
            server_trust: "ca-ref-1",
            client_identity: null,
          },
        },
      },
    });
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
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
    await user.click(screen.getByRole("switch", { name: "转发到固定 Server" }));

    await waitFor(() => expect(mocks.listenerCertificateDiscard).toHaveBeenCalledWith(
      certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust"),
    ));
  });

  it("页面卸载时清理所有尚未保存的导入证书材料", async () => {
    const fixedWorkspace = {
      ...workspace,
      listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    const user = userEvent.setup();
    const { unmount } = render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
    expect(await screen.findByText("CN=测试上游 CA")).toBeVisible();
    unmount();

    await waitFor(() => expect(mocks.listenerCertificateDiscard).toHaveBeenCalledWith(
      certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust"),
    ));
  });

  it("保存请求进行中卸载时等待 Rust 结果且不清理成功保存的证书材料", async () => {
    const fixedWorkspace = {
      ...workspace,
      listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")],
    };
    mocks.workspaceGet.mockReturnValue(ok(fixedWorkspace));
    let resolveSave!: (result: unknown) => void;
    mocks.listenerSave.mockReturnValue(new Promise<unknown>((resolve) => { resolveSave = resolve; }));
    const user = userEvent.setup();
    const { unmount } = render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    unmount();
    expect(mocks.listenerCertificateDiscard).not.toHaveBeenCalled();

    const trust = certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust");
    await act(async () => {
      resolveSave(await ok({
        ...fixedWorkspace,
        revision: 2,
        listeners: [{
          ...fixedWorkspace.listeners[0],
          data_plane: {
            kind: "http" as const,
            settings: {
              ...fixedWorkspace.listeners[0].data_plane.settings,
              fixed_server: {
                ...fixedWorkspace.listeners[0].data_plane.settings.fixed_server!,
                upstream_tls: {
                  ...fixedWorkspace.listeners[0].data_plane.settings.fixed_server!.upstream_tls,
                  server_trust: trust.id,
                },
              },
            },
          },
        }],
        certificate_references: [trust],
      }));
      await Promise.resolve();
    });

    expect(mocks.listenerCertificateDiscard).not.toHaveBeenCalled();
  });

  it("切换工作区时清理前一工作区尚未保存的导入证书材料", async () => {
    const listenerA = fixedListener("listener-a", "监听 A", 16627, "https://a.test:16627");
    const listenerB = dynamicListener("listener-b", "监听 B", 16127);
    const firstWorkspace = { ...workspace, listeners: [listenerA, listenerB] };
    const afterDelete = { ...firstWorkspace, revision: 2, listeners: [listenerA] };
    const secondWorkspace = {
      ...workspace,
      id: "workspace-2",
      name: "第二工作区",
      listeners: [dynamicListener("listener-2", "第二工作区监听", 8082)],
    };
    let deleted = false;
    mocks.workspaceList
      .mockReturnValueOnce(ok([{ id: "workspace-1", name: "API Lab", revision: 1, listener_count: 2, enabled_listener_count: 0, selected: true }]))
      .mockReturnValue(ok([{ id: "workspace-2", name: "第二工作区", revision: 1, listener_count: 1, enabled_listener_count: 0, selected: true }]));
    mocks.workspaceGet.mockImplementation((workspaceId) => ok(
      workspaceId === "workspace-2" ? secondWorkspace : deleted ? afterDelete : firstWorkspace,
    ));
    mocks.listenerDelete.mockImplementation(() => {
      deleted = true;
      return ok({ success: true, cancelled: false, message: "Listener 已删除。", ui_tone: "positive", entity_id: null, revision: 2, requires_restart: false });
    });
    mocks.listenerOverview.mockReturnValue(ok(listenerOverview([
      listenerStatus("listener-a"),
      listenerStatus("listener-b"),
    ])));
    const user = userEvent.setup();
    render(<ListenersView />);

    await user.click(await screen.findByRole("button", { name: "导入 Server CA" }));
    await user.click(screen.getByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));
    await user.click(screen.getByText("监听 B"));
    await user.click(screen.getByRole("button", { name: "删除监听" }));

    expect(await screen.findByText("第二工作区监听")).toBeVisible();
    await waitFor(() => expect(mocks.listenerCertificateDiscard).toHaveBeenCalledWith(
      certificateReference("ca-ref-1", "测试 CA", "upstream_server_trust"),
    ));
  });

  it("解除持久化证书绑定时不删除 Rust 安全存储材料", async () => {
    const trust = certificateReference("persisted-ca", "持久化 CA", "upstream_server_trust");
    const base = fixedListener("fixed-1", "交易", 16627, "https://server.test:443");
    const listener = {
      ...base,
      data_plane: {
        kind: "http" as const,
        settings: {
          ...base.data_plane.settings,
          fixed_server: {
            ...base.data_plane.settings.fixed_server!,
            upstream_tls: {
              ...base.data_plane.settings.fixed_server!.upstream_tls,
              server_trust: trust.id,
            },
          },
        },
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

});
