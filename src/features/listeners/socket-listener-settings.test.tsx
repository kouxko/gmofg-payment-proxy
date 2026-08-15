import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { CertificateReference, SocketRelaySettings } from "@/generated/rust-types";
import { SocketListenerSettings } from "./socket-listener-settings";

const certificates: CertificateReference[] = [
  { id: "app-identity", label: "App Identity", kind: "reverse_server_identity", reference: "managed:app-identity" },
  { id: "app-ca", label: "App Client CA", kind: "downstream_client_trust", reference: "managed:app-ca" },
  { id: "server-ca", label: "Server CA", kind: "upstream_server_trust", reference: "managed:server-ca" },
  { id: "client-identity", label: "Client Identity", kind: "upstream_client_identity", reference: "managed:client-identity" },
];

function relayTlsSettings(): SocketRelaySettings {
  return {
    topology: {
      mode: "relay",
      settings: {
        upstream: { host: "server.test", port: 9443 },
        security: {
          mode: "tls_to_tls",
          downstream_tls: {
            server_identity: "app-identity",
            client_authentication: { mode: "required", trust: "app-ca" },
          },
          upstream_tls: {
            verify_hostname: true,
            server_trust: "server-ca",
            client_identity: "client-identity",
          },
        },
      },
    },
    maximum_connections: 100,
    processing: { mode: "direct" },
  };
}

function localTlsSettings(): SocketRelaySettings {
  return {
    ...relayTlsSettings(),
    topology: {
      mode: "local_responder",
      settings: {
        downstream_security: {
          mode: "tls",
          downstream_tls: {
            server_identity: "app-identity",
            client_authentication: { mode: "required", trust: "app-ca" },
          },
        },
      },
    },
    processing: {
      mode: "scripted",
      settings: {
        package: { id: "iso-8583", version: "1.0.0" },
        upstream: { decode_enabled: true, encode_enabled: false },
        downstream: { decode_enabled: false, encode_enabled: true },
      },
    },
  };
}

function renderSettings(settings: SocketRelaySettings, locked = false, overrides: {
  onChange?: (changes: Partial<SocketRelaySettings>) => void;
  fieldErrors?: Record<string, string[]>;
} = {}) {
  const onChange = overrides.onChange ?? vi.fn<(changes: Partial<SocketRelaySettings>) => void>();
  const view = render(
    <SocketListenerSettings
      settings={settings}
      certificateReferences={certificates}
      certificateDetails={[]}
      protocolCatalog={{
        loading: false,
        refresh: vi.fn().mockResolvedValue(undefined),
        data: { options: [], installed_version_count: 0, unavailable_version_count: 0 },
      }}
      locked={locked}
      fieldErrors={overrides.fieldErrors}
      busy={false}
      testing={false}
      onChange={onChange}
      onImportDownstreamServerIdentity={vi.fn().mockResolvedValue(true)}
      onImportDownstreamClientTrust={vi.fn().mockResolvedValue(true)}
      onImportClientIdentity={vi.fn().mockResolvedValue(true)}
      onImportServerTrust={vi.fn().mockResolvedValue(true)}
      onTest={vi.fn().mockResolvedValue(undefined)}
    />,
  );
  return { ...view, onChange };
}

function SettingsHarness() {
  const [settings, setSettings] = useState(relayTlsSettings());
  return <SocketListenerSettings
    settings={settings}
    certificateReferences={certificates}
    certificateDetails={[]}
    protocolCatalog={{ loading: false, refresh: vi.fn(), data: { options: [], installed_version_count: 0, unavailable_version_count: 0 } }}
    locked={false}
    busy={false}
    testing={false}
    onChange={(changes) => setSettings((current) => ({ ...current, ...changes }))}
    onImportDownstreamServerIdentity={vi.fn().mockResolvedValue(true)}
    onImportDownstreamClientTrust={vi.fn().mockResolvedValue(true)}
    onImportClientIdentity={vi.fn().mockResolvedValue(true)}
    onImportServerTrust={vi.fn().mockResolvedValue(true)}
    onTest={vi.fn().mockResolvedValue(undefined)}
  />;
}

describe("SocketListenerSettings", () => {
  it("renders Relay App TLS, Server TLS and both mTLS certificate selections", () => {
    renderSettings(relayTlsSettings());

    expect(screen.getByLabelText("App 接入传输")).toBeEnabled();
    expect(screen.getByLabelText("App 侧服务端身份")).toBeEnabled();
    expect(screen.getByLabelText("App 客户端证书要求")).toBeEnabled();
    expect(screen.getByLabelText("App 客户端 CA")).toBeEnabled();
    expect(screen.getByLabelText("Server 传输")).toBeEnabled();
    expect(screen.getByLabelText("Server CA")).toBeEnabled();
    expect(screen.getByLabelText("Server mTLS 客户端身份")).toBeEnabled();
  });

  it("renders LocalResponder App TLS and mTLS without any Server, upstream, DNS or test DOM", () => {
    renderSettings(localTlsSettings());

    expect(screen.getByLabelText("App 接入传输")).toBeEnabled();
    expect(screen.getByLabelText("App 侧服务端身份")).toBeEnabled();
    expect(screen.getByLabelText("App 客户端证书要求")).toBeEnabled();
    expect(screen.getByLabelText("App 客户端 CA")).toBeEnabled();
    expect(screen.queryByLabelText("Server 传输")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Socket Server 主机")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /测试 Server|DNS|上游/ })).not.toBeInTheDocument();
  });

  it("locks every certificate Select while a persisted Listener is running or runtime state is unknown", () => {
    renderSettings(relayTlsSettings(), true);

    expect(screen.getByLabelText("App 侧服务端身份")).toBeDisabled();
    expect(screen.getByLabelText("App 客户端证书要求")).toBeDisabled();
    expect(screen.getByLabelText("App 客户端 CA")).toBeDisabled();
    expect(screen.getByLabelText("Server CA")).toBeDisabled();
    expect(screen.getByLabelText("Server mTLS 客户端身份")).toBeDisabled();
  });

  it("keeps all transport, topology and processing controls editable for an unsaved Listener", () => {
    renderSettings(relayTlsSettings(), false);

    expect(screen.getByLabelText("Socket 数据处理模式")).toBeEnabled();
    expect(screen.getByLabelText("Socket 连接拓扑")).toBeEnabled();
    expect(screen.getByLabelText("App 接入传输")).toBeEnabled();
    expect(screen.getByLabelText("Server 传输")).toBeEnabled();
  });

  it("renders a legacy missing processing field as Direct without crashing", () => {
    const legacy = { ...relayTlsSettings(), processing: undefined } as unknown as SocketRelaySettings;

    renderSettings(legacy);

    expect(screen.getByLabelText("Socket 数据处理模式")).toHaveTextContent("Direct");
    expect(screen.getByText("4. Direct 字节转发")).toBeVisible();
  });

  it("switches Relay to LocalResponder and back without hook-order errors or stale Server fields", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const user = userEvent.setup();
    render(<SettingsHarness />);

    await user.click(screen.getByLabelText("Socket 连接拓扑"));
    await user.click(await screen.findByRole("option", { name: "LocalResponder" }));
    expect(screen.queryByLabelText("Socket Server 主机")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Server 传输")).not.toBeInTheDocument();

    await user.click(screen.getByLabelText("Socket 连接拓扑"));
    await user.click(await screen.findByRole("option", { name: "Relay" }));
    expect(screen.getByLabelText("Socket Server 主机")).toHaveValue("");
    expect(screen.getByLabelText("Socket Server 端口")).toHaveValue("0");
    expect(consoleError.mock.calls.flat().join(" ")).not.toMatch(/order of Hooks|Rendered (?:more|fewer) hooks/);
    consoleError.mockRestore();
  });

  it("selects Scripted processing through HeroUI and announces the required package step", async () => {
    const { onChange } = renderSettings(relayTlsSettings());
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("Socket 数据处理模式"));
    await user.click(await screen.findByRole("option", { name: "Scripted" }));

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
      processing: expect.objectContaining({ mode: "scripted" }),
    }));
    expect(screen.getByRole("status")).toHaveTextContent("已切换 Scripted；请选择精确协议包并配置方向能力。");
  });

  it("selects Direct processing through HeroUI and announces the Relay reset", async () => {
    const scripted = localTlsSettings();
    const { onChange } = renderSettings(scripted);
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("Socket 数据处理模式"));
    await user.click(await screen.findByRole("option", { name: "Direct" }));

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ processing: { mode: "direct" } }));
    expect(screen.getByRole("status")).toHaveTextContent("已切换 Direct；已恢复 Relay 并关闭脚本处理。");
  });

  it("submits the maximum connection count from the NumberField", async () => {
    const { onChange } = renderSettings(relayTlsSettings());
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "Increase Socket 最大并发连接" }));

    expect(onChange).toHaveBeenCalledWith({ maximum_connections: 101 });
  });

  it("groups stable Socket field errors without guessing from messages", () => {
    renderSettings(relayTlsSettings(), false, { fieldErrors: {
      "listeners[0].data_plane.settings.topology.upstream": ["endpoint invalid"],
      "listeners[0].data_plane.settings.socket_rules[0].package": ["rule invalid"],
      "listeners[0].data_plane.settings.processing.package": ["package unavailable"],
      "listeners[0].data_plane.settings.processing.downstream.encode_enabled": ["encode unsupported"],
      "listeners[0].data_plane.settings.maximum_connections": ["count invalid"],
      "listeners[0].name": ["outside Socket settings"],
    } });

    expect(screen.getByText("Socket 拓扑需要修正")).toBeVisible();
    expect(screen.getByText("Document 规则需要修正")).toBeVisible();
    expect(screen.getByText("精确协议包需要修正")).toBeVisible();
    expect(screen.getByText("方向能力需要修正")).toBeVisible();
    expect(screen.getByText("Socket 配置需要修正")).toBeVisible();
    expect(screen.queryByText(/outside Socket settings/)).not.toBeInTheDocument();
  });
});
