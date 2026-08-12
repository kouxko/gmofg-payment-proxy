// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
import { ListenerEditor } from "./listener-editor";
import type { ProxyListener } from "@/generated/rust-types";
import { dynamicListener, socketListener } from "./listeners-view.test-support";

function props(
  listener: ProxyListener = socketListener(),
  overrides: Partial<ComponentProps<typeof ListenerEditor>> = {},
): ComponentProps<typeof ListenerEditor> {
  return {
    listener,
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
          upstream: { host: "", port: 0 },
          security: { mode: "transparent" },
          maximum_connections: 500,
        },
      },
    });
  });

  it("Transparent 仅显示目标与容量，不显示任何证书控件", () => {
    render(<ListenerEditor {...props()} />);

    expect(screen.getByRole("textbox", { name: "Socket 上游主机" })).toHaveValue("server.test");
    expect(screen.getByRole("textbox", { name: /Socket 上游端口/ })).toHaveValue("9,443");
    expect(screen.getByText(/双向字节保持 opaque/)).toBeVisible();
    expect(screen.queryByRole("button", { name: /导入.*身份|导入.*CA/ })).not.toBeInTheDocument();
  });

  it.each([
    ["transparent", []],
    ["tcp_to_tls", ["导入上游 Server CA", "导入上游客户端身份"]],
    ["tls_to_tcp", ["导入服务端身份 PEM"]],
    ["tls_to_tls", ["导入服务端身份 PEM", "导入上游 Server CA", "导入上游客户端身份"]],
  ] as const)("%s 只显示该模式真正使用的证书控件", (mode, expectedButtons) => {
    render(<ListenerEditor {...props(socketListener("socket-1", "Socket", 9000, mode))} />);

    const certificateButtons = [
      "导入服务端身份 PEM",
      "导入下游客户端 CA",
      "导入上游 Server CA",
      "导入上游客户端身份",
    ];
    for (const button of certificateButtons) {
      const shouldExist = expectedButtons.includes(button as never);
      expect(Boolean(screen.queryByRole("button", { name: button }))).toBe(shouldExist);
    }
    expect(Boolean(screen.queryByText("客户端 → Relay TLS"))).toBe(
      mode === "tls_to_tcp" || mode === "tls_to_tls",
    );
    expect(Boolean(screen.queryByText("Relay → Server TLS"))).toBe(
      mode === "tcp_to_tls" || mode === "tls_to_tls",
    );
  });

  it.each(["tls_to_tcp", "tls_to_tls"] as const)(
    "%s 配置下游 mTLS 时显示客户端 CA",
    (mode) => {
      const listener = socketListener("socket-1", "Socket", 9000, mode);
      if (listener.data_plane.kind !== "socket") throw new Error("Socket fixture expected");
      const security = listener.data_plane.settings.security;
      if (security.mode !== "tls_to_tcp" && security.mode !== "tls_to_tls") {
        throw new Error("downstream TLS fixture expected");
      }
      const withMtls: ProxyListener = {
        ...listener,
        data_plane: {
          kind: "socket",
          settings: {
            ...listener.data_plane.settings,
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
      };

      render(<ListenerEditor {...props(withMtls)} />);

      expect(screen.getByRole("button", { name: "导入下游客户端 CA" })).toBeVisible();
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
    expect(screen.getByText("桥接：TCP → TLS")).toBeVisible();
  });
});
