// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { ListenerEditor } from "./listener-editor";
import type { ProxyListener } from "@/generated/rust-types";
import {
  dynamicListener,
  localResponderListener,
  socketListener,
} from "./listeners-view.test-support";

function props(
  listener: ProxyListener = socketListener(),
  overrides: Partial<ComponentProps<typeof ListenerEditor>> = {},
): ComponentProps<typeof ListenerEditor> {
  return {
    listener,
    protocolCatalog: { loading: false, refresh: vi.fn(), data: { options: [], installed_version_count: 0, unavailable_version_count: 0, recommended_package: null } },
    locked: false,
    certificateReferences: [],
    certificateDetails: [],
    basicUsername: "",
    basicPassword: "",
    onBasicUsernameChange: vi.fn(),
    onBasicPasswordChange: vi.fn(),
    onChange: vi.fn(),
    onStoreBasicCredential: vi.fn().mockResolvedValue(undefined),
    onImportDownstreamServerIdentity: vi.fn().mockResolvedValue(true),
    onImportDownstreamClientTrust: vi.fn().mockResolvedValue(true),
    onImportClientIdentity: vi.fn().mockResolvedValue(true),
    onImportServerTrust: vi.fn().mockResolvedValue(true),
    onTestUpstreamTls: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("Socket listener editor", () => {
  it("新监听默认为 HTTP，显式切换 Socket 时只提交 Socket variant", async () => {
    const editor = props(dynamicListener());
    const user = userEvent.setup();
    render(<ListenerEditor {...editor} />);

    expect(screen.getByRole("button", { name: /监听数据平面/ })).toHaveTextContent("HTTP");
    await user.click(screen.getByRole("button", { name: /监听数据平面/ }));
    await user.click(await screen.findByRole("option", { name: "Socket 转发" }));

    expect(editor.onChange).toHaveBeenLastCalledWith({
      data_plane: {
        kind: "socket",
        settings: {
          topology: {
            mode: "relay",
            settings: {
              upstream: { host: "", port: 0 },
              security: { mode: "transparent" },
            },
          },
          maximum_connections: 500,
          processing: { mode: "direct" },
        },
      },
    });
  });

  it("Transparent 仅显示目标与容量，不显示任何证书控件", () => {
    render(<ListenerEditor {...props()} />);

    expect(screen.getByRole("textbox", { name: "Socket Server 主机" })).toHaveValue("server.test");
    expect(screen.getByRole("textbox", { name: /Socket Server 端口/ })).toHaveValue("9,443");
    expect(screen.getByText(/应用与上游之间的数据保持原样转发/)).toBeVisible();
    expect(screen.queryByRole("button", { name: /导入.*身份|导入.*CA/ })).not.toBeInTheDocument();
  });

  it.each([
    ["transparent", []],
    ["tcp_to_tls", ["导入 Server CA", "导入客户端身份"]],
    ["tls_to_tcp", ["导入服务端身份"]],
    ["tls_to_tls", ["导入服务端身份", "导入 Server CA", "导入客户端身份"]],
  ] as const)("%s 只显示该模式真正使用的证书控件", (mode, expectedButtons) => {
    render(<ListenerEditor {...props(socketListener("socket-1", "Socket", 9000, mode))} />);

    const certificateButtons = [
      "导入服务端身份",
      "导入客户端 CA",
      "导入 Server CA",
      "导入客户端身份",
    ];
    for (const button of certificateButtons) {
      const shouldExist = expectedButtons.includes(button as never);
      expect(Boolean(screen.queryByRole("button", { name: button }))).toBe(shouldExist);
    }
    expect(Boolean(screen.queryByLabelText("App 侧服务端身份"))).toBe(
      mode === "tls_to_tcp" || mode === "tls_to_tls",
    );
    expect(Boolean(screen.queryByLabelText("Server CA"))).toBe(
      mode === "tcp_to_tls" || mode === "tls_to_tls",
    );
  });

  it.each(["tls_to_tcp", "tls_to_tls"] as const)(
    "%s 配置下游 mTLS 时显示客户端 CA",
    (mode) => {
      const listener = socketListener("socket-1", "Socket", 9000, mode);
      if (listener.data_plane.kind !== "socket") throw new Error("Socket fixture expected");
      const topology = listener.data_plane.settings.topology;
      if (topology.mode !== "relay") throw new Error("Relay fixture expected");
      const security = topology.settings.security;
      if (security.mode !== "tls_to_tcp" && security.mode !== "tls_to_tls") {
        throw new Error("downstream TLS fixture expected");
      }
      const withMtls: ProxyListener = {
        ...listener,
        data_plane: {
          kind: "socket",
          settings: {
            ...listener.data_plane.settings,
            topology: {
              mode: "relay",
              settings: {
                ...topology.settings,
                security: {
                  ...security,
                  downstream_tls: {
                    ...security.downstream_tls,
                    client_authentication: {
                      mode: "required",
                      trust: "downstream-trust-ref",
                    },
                  },
                },
              },
            },
          },
        },
      };

      render(<ListenerEditor {...props(withMtls)} />);

      expect(screen.getByRole("button", { name: "导入客户端 CA" })).toBeVisible();
    },
  );

  it("Socket TLS 探测展示 TCP 与 TLS 证据", () => {
    render(<ListenerEditor {...props(socketListener("socket-1", "Socket", 9000, "tcp_to_tls"), {
      tlsTest: {
        listener_id: "socket-1",
        data_plane: "socket",
        upstream_origin: "server.test:9443",
        resolved_address: "192.0.2.5:9443",
        scheme: "tls",
        transport: "TCP + TLS",
        tls: {
          tls_version: "TLS 1.3",
          cipher_suite: "TLS_AES_128_GCM_SHA256",
          peer_subject: "CN=server.test",
          peer_sha256_fingerprint: "AA:BB",
          hostname_verification_enabled: true,
          client_identity_configured: false,
        },
        socket_transport_mode: "tcp_to_tls",
        elapsed_millis: 9,
        message: "Socket 上游连接成功。",
        ui_tone: "positive",
      },
    })} />);

    expect(screen.getByText(/192\.0\.2\.5:9443 · 9 ms/)).toBeVisible();
    expect(screen.getByText(/TLS 1\.3 · TLS_AES_128_GCM_SHA256/)).toBeVisible();
    expect(screen.getByText("传输：TCP + TLS")).toBeVisible();
  });

  it("LocalResponder 安全渲染且不挂载任何 Server 上游控件", () => {
    const editor = props(localResponderListener() as ProxyListener);
    render(<ListenerEditor {...editor} />);

    expect(screen.getByText("2. App 接入安全")).toBeVisible();
    expect(screen.getByText("4. 协议处理")).toBeVisible();
    expect(screen.queryByRole("textbox", { name: "Socket Server 主机" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "测试 Server 连接" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Server CA|客户端身份/ })).not.toBeInTheDocument();
  });
});
