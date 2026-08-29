import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { CertificateReference, SocketRelaySettings } from "@/generated/rust-types";
import { SocketListenerSettings } from "./socket-listener-settings";
import { defaultSocketRuntimeLimits } from "./listener-data-plane";
import type { ProtocolCatalogState } from "./socket-processing-card";

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
            tls_server_name: null,
            server_trust: "server-ca",
            client_identity: "client-identity",
          },
        },
      },
    },
    maximum_connections: 100,
    runtime_limits: defaultSocketRuntimeLimits(),
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
      },
    },
  };
}

function protocolRelaySettings(): SocketRelaySettings {
  return {
    ...localTlsSettings(),
    topology: relayTlsSettings().topology,
  };
}

function renderSettings(settings: SocketRelaySettings, locked = false, overrides: {
  onChange?: (changes: Partial<SocketRelaySettings>) => void;
  fieldErrors?: Record<string, string[]>;
  protocolCatalog?: ProtocolCatalogState;
} = {}) {
  const onChange = overrides.onChange ?? vi.fn<(changes: Partial<SocketRelaySettings>) => void>();
  const view = render(
    <SocketListenerSettings
      settings={settings}
      certificateReferences={certificates}
      certificateDetails={[]}
      protocolCatalog={overrides.protocolCatalog ?? {
        loading: false,
        refresh: vi.fn().mockResolvedValue(undefined),
        data: { options: [], installed_version_count: 0, unavailable_version_count: 0, recommended_package: null },
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
    protocolCatalog={{ loading: false, refresh: vi.fn(), data: { options: [], installed_version_count: 0, unavailable_version_count: 0, recommended_package: null } }}
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

    expect(screen.getByLabelText("Socket 响应方式")).toBeEnabled();
    expect(screen.getByLabelText("Socket 协议处理方案")).toBeEnabled();
    expect(screen.getByLabelText("App 接入传输")).toBeEnabled();
    expect(screen.getByLabelText("Server 传输")).toBeEnabled();
  });

  it("switches protocol relay to local response and back without hook-order errors or stale Server fields", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const user = userEvent.setup();
    render(<SettingsHarness />);

    await user.click(screen.getByLabelText("Socket 响应方式"));
    await user.click(await screen.findByRole("option", { name: "本机应答" }));
    expect(screen.queryByLabelText("Socket Server 主机")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Server 传输")).not.toBeInTheDocument();

    await user.click(screen.getByLabelText("Socket 响应方式"));
    await user.click(await screen.findByRole("option", { name: "转发到上游" }));
    expect(screen.getByLabelText("Socket Server 主机")).toHaveValue("");
    expect(screen.getByLabelText("Socket Server 端口")).toHaveValue("0");
    expect(consoleError.mock.calls.flat().join(" ")).not.toMatch(/order of Hooks|Rendered (?:more|fewer) hooks/);
    consoleError.mockRestore();
  });

  it("selects a protocol package through HeroUI as the full processing chain", async () => {
    const { onChange } = renderSettings(relayTlsSettings(), false, { protocolCatalog: {
      loading: false,
      refresh: vi.fn(),
      data: { options: [], installed_version_count: 0, unavailable_version_count: 0, recommended_package: null },
    } });
    expect(screen.getByLabelText("Socket 协议处理方案")).toBeEnabled();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("uses the catalog recommendation only when first entering protocol processing", async () => {
    const recommended = { id: "iso8583-ascii-standard", version: "1.0.0" };
    const option = {
      package: recommended,
      name: "ISO 8583 ASCII 示例",
      package_source: { type: "internal" as const, built_in: true },
      kind: "socket" as const,
      capabilities: {
        upstream: { frame: true, decode: true, encode: true },
        downstream: { frame: true, decode: true, encode: true },
        display: true,
      },
      upstream_schema: {
        root: { type: "object" as const, title: "ISO 8583 Request", properties: { mti: { type: "string" as const, title: "MTI" } } },
      },
      downstream_schema: {
        root: { type: "object" as const, title: "ISO 8583 Response", properties: { response_code: { type: "string" as const, title: "Response" } } },
      },
    };
    const { onChange } = renderSettings(relayTlsSettings(), false, {
      protocolCatalog: {
        loading: false,
        refresh: vi.fn().mockResolvedValue(undefined),
        data: {
          options: [option],
          installed_version_count: 1,
          unavailable_version_count: 0,
          recommended_package: recommended,
        },
      },
    });
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("Socket 响应方式"));
    await user.click(await screen.findByRole("option", { name: "本机应答" }));

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
      processing: expect.objectContaining({
        mode: "scripted",
        settings: expect.objectContaining({ package: recommended }),
      }),
    }));
  });

  it("selects raw relay through HeroUI and announces that protocol processing was cleared", async () => {
    const scripted = protocolRelaySettings();
    const { onChange } = renderSettings(scripted);
    const user = userEvent.setup();

    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    await user.click(await screen.findByRole("option", { name: /不使用协议包/ }));

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ processing: { mode: "direct" } }));
    expect(screen.getByRole("status")).toHaveTextContent("已取消协议包；数据将保持原样透明转发。");
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
      "listeners[0].data_plane.settings.protocol_rules[0].package": ["rule invalid"],
      "listeners[0].data_plane.settings.processing.package": ["package unavailable"],
      "listeners[0].data_plane.settings.processing": ["processing unavailable"],
      "listeners[0].data_plane.settings.maximum_connections": ["count invalid"],
      "listeners[0].name": ["outside Socket settings"],
    } });

    expect(screen.getByText("Socket 连接需要修正")).toBeVisible();
    expect(screen.getByText("内容处理规则需要修正")).toBeVisible();
    expect(screen.getByText("精确协议包需要修正")).toBeVisible();
    expect(screen.getByText("协议处理需要修正")).toBeVisible();
    expect(screen.getByText("Socket 配置需要修正")).toBeVisible();
    expect(screen.queryByText(/outside Socket settings/)).not.toBeInTheDocument();
  });

  it.each([
    ["透明转发", relayTlsSettings(), true, false],
    ["按协议转发", protocolRelaySettings(), true, true],
    ["本地应答", localTlsSettings(), false, true],
  ] as const)("renders only the cards required by %s", (_label, settings, server, protocol) => {
    renderSettings(settings);

    expect(screen.queryByLabelText("Socket Server 主机") !== null).toBe(server);
    expect(screen.queryByLabelText("Socket 协议处理方案")).toBeInTheDocument();
    expect(screen.queryByText("当前未使用协议包，应用与上游之间的数据保持原样转发。") !== null).toBe(!protocol);
  });

  it.each([
    relayTlsSettings(),
    protocolRelaySettings(),
    localTlsSettings(),
  ])("keeps all nine implementation terms out of the ordinary configuration DOM", (settings) => {
    const { container } = renderSettings(settings);
    const ordinary = container.cloneNode(true) as HTMLElement;
    ordinary.querySelectorAll("details").forEach((details) => details.remove());
    const accessibleText = [ordinary.textContent ?? "", ...Array.from(
      ordinary.querySelectorAll<HTMLElement>("[aria-label], [title], [placeholder]"),
      (element) => [
        element.getAttribute("aria-label"),
        element.getAttribute("title"),
        element.getAttribute("placeholder"),
      ].filter(Boolean).join(" "),
    )].join(" ");

    expect(accessibleText).not.toMatch(
      /\b(?:Direct|Scripted|Relay|LocalResponder|Frame|Document|Decode|Encode|Display)\b/i,
    );
  });

  it("locks the work mode and every mounted snapshot control", () => {
    renderSettings(protocolRelaySettings(), true);

    expect(screen.getByLabelText("Socket 响应方式")).toBeDisabled();
    expect(screen.getByLabelText("Socket 最大并发连接")).toBeDisabled();
    expect(screen.getByLabelText("App 接入传输")).toBeDisabled();
    expect(screen.getByLabelText("Socket Server 主机")).toBeDisabled();
    expect(screen.getByLabelText("Socket 协议处理方案")).toBeDisabled();
    expect(screen.getAllByRole("switch").every((control) => control.hasAttribute("disabled")))
      .toBe(true);
  });
});
