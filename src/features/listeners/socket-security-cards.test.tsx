import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  CertificateReference,
  ListenerUpstreamConnectionTestViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import { SocketAppSecurityCard, SocketServerCard } from "./socket-security-cards";
const references: CertificateReference[] = [
  { id: "app-id", label: "App Identity", kind: "reverse_server_identity", reference: "managed:app-id" },
  { id: "app-ca", label: "App CA", kind: "downstream_client_trust", reference: "managed:app-ca" },
  { id: "server-ca", label: "Server CA", kind: "upstream_server_trust", reference: "managed:server-ca" },
  { id: "client-id", label: "Client Identity", kind: "upstream_client_identity", reference: "managed:client-id" },
];

function relay(security: "transparent" | "tls_to_tls" = "tls_to_tls"): SocketRelaySettings {
  return {
    topology: {
      mode: "relay",
      settings: {
        upstream: { host: "server.test", port: 9443 },
        security: security === "transparent" ? { mode: "transparent" } : {
          mode: "tls_to_tls",
          downstream_tls: {
            server_identity: "app-id",
            client_authentication: { mode: "required", trust: "app-ca" },
          },
          upstream_tls: {
            verify_hostname: true,
            tls_server_name: null,
            server_trust: "server-ca",
            client_identity: "client-id",
          },
        },
      },
    },
    maximum_connections: 32,
    processing: { mode: "direct" },
  };
}

function local(): SocketRelaySettings {
  return {
    ...relay(),
    topology: { mode: "local_responder", settings: { downstream_security: { mode: "tcp" } } },
  };
}

function common(settings: SocketRelaySettings, overrides: { locked?: boolean; busy?: boolean } = {}) {
  return {
    settings,
    certificateReferences: references,
    certificateDetails: references.map((reference) => ({
      reference_id: reference.id, label: reference.label, certificate: null, error_message: null,
    })),
    locked: overrides.locked ?? false,
    busy: overrides.busy ?? false,
    onChange: vi.fn(),
  };
}

function renderApp(settings = relay(), overrides: { locked?: boolean; busy?: boolean } = {}) {
  const props = {
    ...common(settings, overrides),
    onImportIdentity: vi.fn().mockResolvedValue(true),
    onImportTrust: vi.fn().mockResolvedValue(true),
  };
  render(<SocketAppSecurityCard {...props} />);
  return props;
}

function renderServer(settings = relay(), overrides: {
  locked?: boolean;
  busy?: boolean;
  testing?: boolean;
  testResult?: ListenerUpstreamConnectionTestViewModel;
  testError?: string;
} = {}) {
  const props = {
    ...common(settings, overrides),
    testing: overrides.testing ?? false,
    testResult: overrides.testResult,
    testError: overrides.testError,
    onImportIdentity: vi.fn().mockResolvedValue(true),
    onImportTrust: vi.fn().mockResolvedValue(true),
    onTest: vi.fn().mockResolvedValue(undefined),
  };
  render(<SocketServerCard {...props} />);
  return props;
}

function testResult(tls = true): ListenerUpstreamConnectionTestViewModel {
  return {
    listener_id: "listener", data_plane: "socket", upstream_origin: "server.test:9443",
    resolved_address: "192.0.2.1:9443", scheme: tls ? "tls" : "tcp", transport: tls ? "TLS" : "TCP",
    tls: tls ? {
      tls_version: "TLSv1.3", cipher_suite: "TLS_AES_256_GCM_SHA384", peer_subject: "CN=server.test",
      peer_sha256_fingerprint: "AA:BB", hostname_verification_enabled: true, client_identity_configured: true,
    } : null,
    socket_transport_mode: tls ? "tls_to_tls" : "transparent", tls_server_name_candidates: [], elapsed_millis: 12, message: "连接成功", ui_tone: "positive",
  };
}

describe("Socket security cards", () => {
  it("renders App TCP without certificate controls", () => {
    renderApp(relay("transparent"));

    expect(screen.getByLabelText("App 接入传输")).toHaveTextContent("TCP");
    expect(screen.queryByLabelText("App 侧服务端身份")).not.toBeInTheDocument();
  });

  it("changes App transport through the HeroUI Select", async () => {
    const props = renderApp(relay("transparent"));
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 接入传输"));
    await user.click(await screen.findByRole("option", { name: "TLS" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({ security: expect.objectContaining({ mode: "tls_to_tcp" }) }) }),
    }));
  });

  it("changes the App service identity through the certificate Select", async () => {
    const props = renderApp(relay());
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 侧服务端身份"));
    await user.click(await screen.findByRole("option", { name: "请选择服务端 PEM identity" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({ server_identity: "" }) }),
      }) }),
    }));
  });

  it("changes App client authentication to optional", async () => {
    const props = renderApp(relay());
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 客户端证书要求"));
    await user.click(await screen.findByRole("option", { name: "客户端证书可选" }));
    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({
          client_authentication: { mode: "optional", trust: "app-ca" },
        }) }),
      }) }),
    }));
  });

  it("disables App client authentication through the HeroUI Select", async () => {
    const props = renderApp(relay());
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 客户端证书要求"));
    await user.click(await screen.findByRole("option", { name: "不要求客户端证书" }));

    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({
          client_authentication: { mode: "disabled" },
        }) }),
      }) }),
    }));
  });

  it("enables required App client authentication using the first available trust", async () => {
    const current = relay();
    if (current.topology.mode !== "relay" || current.topology.settings.security.mode !== "tls_to_tls") throw new Error("expected TLS Relay");
    current.topology.settings.security.downstream_tls.client_authentication = { mode: "disabled" };
    const props = renderApp(current);
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 客户端证书要求"));
    await user.click(await screen.findByRole("option", { name: "必须验证客户端证书" }));

    expect(props.onChange).toHaveBeenLastCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({
          client_authentication: { mode: "required", trust: "app-ca" },
        }) }),
      }) }),
    }));
  });

  it("changes the required App client CA through the certificate Select", async () => {
    const props = renderApp(relay());
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 客户端 CA"));
    await user.click(await screen.findByRole("option", { name: "请选择客户端 CA" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({
          client_authentication: { mode: "required", trust: "" },
        }) }),
      }) }),
    }));
  });

  it("changes the optional App client CA without changing authentication mode", async () => {
    const current = relay();
    if (current.topology.mode !== "relay" || current.topology.settings.security.mode !== "tls_to_tls") throw new Error("expected TLS Relay");
    current.topology.settings.security.downstream_tls.client_authentication = { mode: "optional", trust: "app-ca" };
    const props = renderApp(current);
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 客户端 CA"));
    await user.click(await screen.findByRole("option", { name: "请选择客户端 CA" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({
          client_authentication: { mode: "optional", trust: "" },
        }) }),
      }) }),
    }));
  });

  it("uses an empty trust when enabling mTLS without any App CA candidates", async () => {
    const current = relay();
    if (current.topology.mode !== "relay" || current.topology.settings.security.mode !== "tls_to_tls") throw new Error("expected TLS Relay");
    current.topology.settings.security.downstream_tls.client_authentication = { mode: "disabled" };
    const props = {
      ...common(current), certificateReferences: references.filter((item) => item.kind !== "downstream_client_trust"),
      onImportIdentity: vi.fn().mockResolvedValue(true), onImportTrust: vi.fn().mockResolvedValue(true),
    };
    render(<SocketAppSecurityCard {...props} />);
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("App 客户端证书要求"));
    await user.click(await screen.findByRole("option", { name: "必须验证客户端证书" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({
        security: expect.objectContaining({ downstream_tls: expect.objectContaining({
          client_authentication: { mode: "required", trust: "" },
        }) }),
      }) }),
    }));
  });

  it("imports an App service identity and closes the modal after success", async () => {
    const props = renderApp();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入服务端身份" }));
    const input = await screen.findByRole("textbox", { name: "显示名称" });
    await user.clear(input); await user.type(input, "Imported App Identity");
    await user.type(screen.getByLabelText("P12 / PFX 密码（PEM 不使用；允许为空）"), "app-secret");
    expect(screen.getByText(/必须具备 serverAuth，而不是.*clientAuth/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "选择服务端身份（.p12 / .pfx / .pem）" }));

    expect(props.onImportIdentity).toHaveBeenCalledWith("Imported App Identity", "app-secret");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入 App 侧服务端身份" })).not.toBeInTheDocument());
    expect(document.body).not.toHaveTextContent("app-secret");
  });

  it("allows an empty App identity password", async () => {
    const props = renderApp();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入服务端身份" }));
    await user.click(await screen.findByRole("button", { name: "选择服务端身份（.p12 / .pfx / .pem）" }));

    expect(props.onImportIdentity).toHaveBeenCalledWith("Socket App 服务端身份", "");
  });

  it("clears the App identity password after a rejected import", async () => {
    const props = renderApp();
    props.onImportIdentity.mockResolvedValue(false);
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入服务端身份" }));
    const password = await screen.findByLabelText("P12 / PFX 密码（PEM 不使用；允许为空）");
    await user.type(password, "rejected-secret");
    await user.click(screen.getByRole("button", { name: "选择服务端身份（.p12 / .pfx / .pem）" }));

    expect(screen.getByRole("dialog", { name: "导入 App 侧服务端身份" })).toBeVisible(); expect(password).toHaveValue("");
    expect(document.body).not.toHaveTextContent("rejected-secret");
  });

  it("locks duplicate App identity imports while the picker result is pending", async () => {
    let resolveImport!: (value: boolean) => void;
    const props = renderApp();
    props.onImportIdentity.mockReturnValue(new Promise<boolean>((resolve) => { resolveImport = resolve; }));
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入服务端身份" }));
    const choose = await screen.findByRole("button", { name: "选择服务端身份（.p12 / .pfx / .pem）" });
    await user.click(choose);
    fireEvent.click(choose);

    expect(props.onImportIdentity).toHaveBeenCalledTimes(1);
    expect(choose).toBeDisabled();
    resolveImport(true);
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入 App 侧服务端身份" })).not.toBeInTheDocument());
  });

  it("keeps the App trust modal open when import is rejected", async () => {
    const props = renderApp();
    props.onImportTrust.mockResolvedValue(false);
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入客户端 CA" }));
    await user.click(await screen.findByRole("button", { name: "选择客户端 CA" }));

    expect(props.onImportTrust).toHaveBeenCalledWith("Socket App 客户端 CA");
    expect(screen.getByRole("dialog", { name: "导入 App 客户端 CA" })).toBeVisible();
  });

  it("closes the App import modal without invoking import", async () => {
    const props = renderApp();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入服务端身份" }));
    await user.type(
      await screen.findByLabelText("P12 / PFX 密码（PEM 不使用；允许为空）"),
      "cancel-secret",
    );
    await user.click(await screen.findByRole("button", { name: "取消" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入 App 侧服务端身份" })).not.toBeInTheDocument());
    expect(props.onImportIdentity).not.toHaveBeenCalled(); expect(document.body).not.toHaveTextContent("cancel-secret");
    await user.click(screen.getByRole("button", { name: "导入服务端身份" }));
    expect(await screen.findByLabelText("P12 / PFX 密码（PEM 不使用；允许为空）")).toHaveValue("");
  });

  it("renders disabled App TLS without a client trust selector", () => {
    const current = relay();
    if (current.topology.mode !== "relay" || current.topology.settings.security.mode !== "tls_to_tls") throw new Error("expected TLS Relay");
    current.topology.settings.security.downstream_tls.server_identity = "";
    current.topology.settings.security.downstream_tls.client_authentication = { mode: "disabled" };

    renderApp(current);

    expect(screen.getByText("尚未选择 App 侧服务端身份。")).toBeVisible();
    expect(screen.queryByLabelText("App 客户端 CA")).not.toBeInTheDocument();
  });

  it("does not render a Server card for LocalResponder", () => {
    const { container } = render(<SocketServerCard {...renderlessServerProps(local())} />);

    expect(container).toBeEmptyDOMElement();
  });

  it("edits the Relay endpoint and starts the Server connection test", async () => {
    const props = renderServer(relay("transparent"));
    const user = userEvent.setup();

    fireEvent.change(screen.getByLabelText("Socket Server 主机"), { target: { value: "next.test" } });
    await user.click(screen.getByRole("button", { name: "Increase Socket Server 端口" }));
    await user.click(screen.getByRole("button", { name: "测试 Server 连接" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({ topology: expect.objectContaining({ settings: expect.objectContaining({ upstream: { host: "next.test", port: 9443 } }) }) }));
    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({ topology: expect.objectContaining({ settings: expect.objectContaining({ upstream: { host: "server.test", port: 9444 } }) }) }));
    expect(props.onTest).toHaveBeenCalledTimes(1);
  });

  it("changes Server transport through the HeroUI Select", async () => {
    const props = renderServer(relay("transparent"));
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("Server 传输"));
    await user.click(await screen.findByRole("option", { name: "TLS" }));

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({ settings: expect.objectContaining({ security: expect.objectContaining({ mode: "tcp_to_tls" }) }) }),
    }));
  });

  it("updates Server TLS verification and certificate selections", async () => {
    const props = renderServer();
    const user = userEvent.setup();

    await user.click(screen.getByRole("switch", { name: "校验 Socket Server 主机名" }));
    await user.click(screen.getByLabelText("Server CA"));
    await user.click(await screen.findByRole("option", { name: "使用系统信任根" }));
    await user.click(screen.getByLabelText("Server mTLS 客户端身份"));
    await user.click(await screen.findByRole("option", { name: "不提供客户端身份" }));

    expect(props.onChange).toHaveBeenCalledTimes(3);
  });

  it("keeps an IP endpoint while editing the TLS Server Name", () => {
    const current = relay();
    if (current.topology.mode !== "relay") throw new Error("expected Relay");
    current.topology.settings.upstream.host = "113.197.126.77";
    const props = renderServer(current);

    fireEvent.change(screen.getByLabelText("Socket TLS Server Name"), {
      target: { value: "testssl.tnsi.com.au" },
    });

    expect(props.onChange).toHaveBeenCalledWith(expect.objectContaining({
      topology: expect.objectContaining({
        settings: expect.objectContaining({
          upstream: { host: "113.197.126.77", port: 9443 },
          security: expect.objectContaining({
            upstream_tls: expect.objectContaining({ tls_server_name: "testssl.tnsi.com.au" }),
          }),
        }),
      }),
    }));
  });

  it("imports a Server client identity with its password", async () => {
    const props = renderServer();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入客户端身份" }));
    await user.type(await screen.findByLabelText("P12 / PFX 密码（PEM 不使用；允许为空）"), "secret");
    await user.click(screen.getByRole("button", { name: "选择客户端身份（.p12 / .pfx / .pem）" }));

    expect(props.onImportIdentity).toHaveBeenCalledWith("Socket Server 客户端身份", "secret");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入 Proxy → Server 的客户端身份" })).not.toBeInTheDocument());
  });

  it("keeps the Server identity modal open when import is rejected", async () => {
    const props = renderServer();
    props.onImportIdentity.mockResolvedValue(false);
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入客户端身份" }));
    await user.click(await screen.findByRole("button", { name: "选择客户端身份（.p12 / .pfx / .pem）" }));

    expect(screen.getByRole("dialog", { name: "导入 Proxy → Server 的客户端身份" })).toBeVisible();
  });

  it("imports Server trust and closes the modal after success", async () => {
    const props = renderServer();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(await screen.findByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));

    expect(props.onImportTrust).toHaveBeenCalledWith("Socket Server CA");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入用于验证上游 Server 的 CA" })).not.toBeInTheDocument());
  });

  it("keeps the Server trust modal open when import is rejected", async () => {
    const props = renderServer();
    props.onImportTrust.mockResolvedValue(false);
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(await screen.findByRole("button", { name: "选择 CA 证书（.cer / .crt / .pem / .der）" }));

    expect(screen.getByRole("dialog", { name: "导入用于验证上游 Server 的 CA" })).toBeVisible();
  });

  it("closes the Server trust modal without invoking import", async () => {
    const props = renderServer();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入 Server CA" }));
    await user.click(await screen.findByRole("button", { name: "取消" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入用于验证上游 Server 的 CA" })).not.toBeInTheDocument());
    expect(props.onImportTrust).not.toHaveBeenCalled();
  });

  it("closes the Server identity import modal without invoking import", async () => {
    const props = renderServer();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "导入客户端身份" }));
    await user.click(await screen.findByRole("button", { name: "取消" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入 Proxy → Server 的客户端身份" })).not.toBeInTheDocument());
    expect(props.onImportIdentity).not.toHaveBeenCalled();
  });

  it("renders system trust and no client identity when Server TLS references are empty", () => {
    const current = relay();
    if (current.topology.mode !== "relay" || current.topology.settings.security.mode !== "tls_to_tls") throw new Error("expected TLS Relay");
    current.topology.settings.security.upstream_tls.server_trust = null;
    current.topology.settings.security.upstream_tls.client_identity = null;

    renderServer(current);

    expect(screen.getByText("当前使用系统信任根。")).toBeVisible();
    expect(screen.getByText("当前不提供 mTLS 客户端身份。")).toBeVisible();
  });

  it("shows pending, successful TLS evidence and connection errors", () => {
    renderServer(relay(), { testing: true, testResult: testResult(), testError: "handshake rejected" });

    expect(screen.getByRole("button", { name: "正在探测 Server…" })).toBeVisible();
    expect(screen.getByText("连接成功")).toBeVisible();
    expect(screen.getByText(/TLSv1\.3/)).toBeVisible();
    expect(screen.getByText("Server 连接失败")).toBeVisible();
    expect(screen.getByText("handshake rejected")).toBeVisible();
  });

  it("hides TLS evidence for a successful TCP connection", () => {
    renderServer(relay("transparent"), { testResult: testResult(false) });

    expect(screen.getByText("传输：TCP")).toBeVisible();
    expect(screen.queryByText(/协商：/)).not.toBeInTheDocument();
  });

  it("disables transport, certificate, import and test controls while busy or locked", () => {
    renderServer(relay(), { locked: true, busy: true });

    expect(screen.getByLabelText("Server 传输")).toBeDisabled();
    expect(screen.getByLabelText("Server CA")).toBeDisabled();
    expect(screen.getByRole("button", { name: "导入 Server CA" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "测试 Server 连接" })).toBeDisabled();
  });
});

function renderlessServerProps(settings: SocketRelaySettings) {
  return {
    ...common(settings), testing: false, onImportIdentity: vi.fn().mockResolvedValue(true),
    onImportTrust: vi.fn().mockResolvedValue(true), onTest: vi.fn().mockResolvedValue(undefined),
  };
}
