// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { bootstrap, setupListenerMocks, mocks, workspace, fixedListener, certificateReference, certificateDetail, ok, navigationMocks, withHttpSettings } from "./listeners-view.test-support";

vi.mock("@/features/shell/workspace-navigation", () => ({ useWorkspaceNavigation: () => navigationMocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: () => undefined,
  useBootstrap: () => ({ bootstrap }),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

import { ListenersView } from "./listeners-view";

describe("统一代理监听编辑器", () => {
  beforeEach(setupListenerMocks);

  it("在当前监听内展示 Rust 解析的证书主题、SAN、有效期和指纹", async () => {
    const serverIdentity = certificateReference("server-ref", "本入口服务端身份", "reverse_server_identity");
    const clientTrust = certificateReference("client-ca-ref", "客户端证书 CA", "downstream_client_trust");
    const listener = withHttpSettings(
      fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
      {
      downstream_tls: {
        enabled: true,
        server_identity: serverIdentity.id,
        dynamic_sni_allowlist: [],
        client_authentication: { mode: "required" as const, trust: clientTrust.id },
      },
    },
    );
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
      listeners: [withHttpSettings(
        fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        {
        downstream_tls: {
          enabled: true,
          server_identity: null,
          dynamic_sni_allowlist: ["api.example.test"],
          client_authentication: { mode: "disabled" as const },
        },
      },
      )],
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
      listeners: [withHttpSettings(
        fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        {
        downstream_tls: {
          enabled: true,
          server_identity: staleIdentity.id,
          dynamic_sni_allowlist: [],
          client_authentication: { mode: "disabled" as const },
        },
      },
      )],
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
    expect(
      mocks.listenerSave.mock.calls[0][2].data_plane.settings.downstream_tls.server_identity,
    ).toBeNull();
  });

  it("导入独立下游身份后只保存受保护引用并显示解析详情", async () => {
    mocks.workspaceGet.mockReturnValue(ok({
      ...workspace,
      listeners: [withHttpSettings(
        fixedListener("fixed-1", "交易", 16627, "https://server.test:443"),
        {
        downstream_tls: {
          enabled: true,
          server_identity: null,
          client_authentication: { mode: "disabled" as const },
        },
      },
      )],
    }));

    const user = userEvent.setup();
    render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入独立服务端身份" }));
    await user.click(screen.getByRole("button", { name: "选择服务端身份 PEM" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));

    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(
      mocks.listenerSave.mock.calls[0][2].data_plane.settings.downstream_tls.server_identity,
    ).toBe("downstream-identity-ref-1");
    expect(mocks.listenerSave.mock.calls[0][3][0].reference).toBe("managed:downstream-identity-ref-1");
    expect(await screen.findByText("CN=proxy.test")).toBeVisible();
  });

  it("导入 mTLS 身份时密码不进入 Workspace", async () => {
    mocks.workspaceGet.mockReturnValue(ok({ ...workspace, listeners: [fixedListener("fixed-1", "交易", 16627, "https://server.test:443")] }));
    const user = userEvent.setup(); render(<ListenersView />);
    await user.click(await screen.findByRole("button", { name: "导入客户端身份" }));
    expect(screen.getByText(/client\.p12 \/ client\.pfx.*client\.pem/)).toBeVisible();
    await user.type(await screen.findByLabelText("P12 / PFX 密码（PEM 不使用；允许为空）"), "p12-secret");
    await user.click(screen.getByRole("button", { name: "选择客户端身份（.p12 / .pfx / .pem）" }));
    await user.click(screen.getByRole("button", { name: "保存当前监听" }));
    await waitFor(() => expect(mocks.listenerSave).toHaveBeenCalledTimes(1));
    expect(
      mocks.listenerSave.mock.calls[0][2].data_plane.settings.fixed_server.upstream_tls.client_identity,
    ).toBe("identity-ref-1");
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
    expect(
      mocks.listenerSave.mock.calls[0][2].data_plane.settings.fixed_server.upstream_url,
    ).toBe("https://dll.test:16127");
    expect(mocks.listenerSave.mock.calls[0][2].id).toBe("dll");
  });

  it("保存监听时只提交该监听实际引用的证书材料", async () => {
    const downstreamIdentity = certificateReference("transaction-downstream", "交易入口服务端身份", "reverse_server_identity");
    const upstreamTrust = certificateReference("transaction-upstream-ca", "交易上游 CA", "upstream_server_trust");
    const otherIdentity = certificateReference("dll-downstream", "DLL 入口服务端身份", "reverse_server_identity");
    const transaction = withHttpSettings(
      fixedListener("transaction", "Transaction", 16627, "https://transaction.test:16627"),
      {
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
    });
    const dll = withHttpSettings(
      fixedListener("dll", "DLL", 16127, "https://dll.test:16127"),
      {
      downstream_tls: {
        enabled: true,
        server_identity: otherIdentity.id,
        client_authentication: { mode: "disabled" as const },
      },
    });
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

});
