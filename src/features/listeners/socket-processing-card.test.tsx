import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import { SocketProcessingCard, type ProtocolCatalogState } from "./socket-processing-card";
import { defaultSocketRuntimeLimits } from "./listener-data-plane";

vi.mock("./socket-protocol-package-dialog", () => ({
  SocketProtocolPackageDialog: ({ packageRef }: { packageRef: { id: string; version: string } }) => (
    <button>查看详情 {packageRef.id}@{packageRef.version}</button>
  ),
}));

function packageOption(
  version = "1.0.0",
  overrides: Partial<ListenerProtocolPackageOptionViewModel> = {},
): ListenerProtocolPackageOptionViewModel {
  return {
    package: { id: "iso-8583", version },
    name: `ISO 8583 ${version}`,
    package_source: { type: "external", online: true },
    kind: "socket",
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: true },
      display: true,
    },
    upstream_schema: {
      root: { type: "object", title: "ISO Request", properties: { mti: { type: "string", title: "MTI" } } },
    },
    downstream_schema: {
      root: { type: "object", title: "ISO Response", properties: { response_code: { type: "string", title: "Response" } } },
    },
    ...overrides,
  };
}

function catalog(
  options: ListenerProtocolPackageOptionViewModel[] = [packageOption()],
  overrides: Partial<ListenerProtocolPackageCatalogViewModel> = {},
): ProtocolCatalogState {
  return {
    loading: false,
    refresh: vi.fn().mockResolvedValue(undefined),
    data: {
      options,
      installed_version_count: options.length,
      unavailable_version_count: 0,
      recommended_package: null,
      ...overrides,
    },
  };
}

function settings(mode: "relay" | "local_responder" = "relay"): SocketRelaySettings {
  return {
    topology: mode === "relay"
      ? {
        mode: "relay",
        settings: {
          upstream: { host: "server.test", port: 9000 },
          security: { mode: "transparent" },
        },
      }
      : {
        mode: "local_responder",
        settings: { downstream_security: { mode: "tcp" } },
      },
    maximum_connections: 32,
    runtime_limits: defaultSocketRuntimeLimits(),
    processing: {
      mode: "scripted",
      settings: {
        package: { id: "iso-8583", version: "1.0.0" },
      },
    },
  };
}

describe("SocketProcessingCard", () => {
  it("represents transparent relay as the absence of a protocol package", async () => {
    const direct = settings();
    direct.processing = { mode: "direct" };
    const user = userEvent.setup();

    render(<SocketProcessingCard settings={direct} catalog={catalog()} locked={false} onChange={vi.fn()} />);

    expect(screen.getByText("当前未使用协议包，应用与上游之间的数据保持原样转发。")).toBeVisible();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    expect(await screen.findByRole("option", { name: "不使用协议包（透明转发）" })).toBeVisible();
  });

  it("automatically applies protocol capabilities and hides identity in advanced details", async () => {
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={settings()} catalog={catalog()} locked={false} onChange={vi.fn()} />);

    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.getByText(/双向数据会自动解析为字段/)).toBeVisible();
    const details = screen.getByText("高级技术信息").closest("details");
    expect(details).not.toHaveAttribute("open");
    await user.click(screen.getByText("高级技术信息"));
    expect(screen.getByText("iso-8583@1.0.0", { selector: "span" })).toBeVisible();
    expect(screen.getByText("上行字段结构 ISO Request")).toBeVisible();
    expect(screen.getByText("下行字段结构 ISO Response")).toBeVisible();
    expect(screen.getByText("报文边界与字段解析：双向支持")).toBeVisible();
    expect(screen.getByText("报文重建：上行 支持，下行 支持")).toBeVisible();
    expect(screen.getByText("协议视图：支持")).toBeVisible();
  });

  it("automatically parses requests and encodes local responses", () => {
    render(
      <SocketProcessingCard
        settings={settings("local_responder")}
        catalog={catalog()}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText(/应用请求会自动解析为字段/)).toBeVisible();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /透明转发/ })).not.toBeInTheDocument();
  });

  it("selecting a package persists only its exact identity and enables the full chain", async () => {
    const direct = settings();
    direct.processing = { mode: "direct" };
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={direct} catalog={catalog()} locked={false} onChange={onChange} />);

    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    await user.click(await screen.findByRole("option", { name: "ISO 8583 1.0.0 · 1.0.0 · 外部 · 在线" }));

    expect(onChange).toHaveBeenCalledWith({
      ...direct,
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
        },
      },
    });
    expect(screen.getByRole("status")).toHaveTextContent("完整的分帧、解析、规则、编码和显示处理链将自动应用");
  });

  it("removing a relay package returns to exact transparent processing", async () => {
    const current = settings();
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={current} catalog={catalog()} locked={false} onChange={onChange} />);

    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    await user.click(await screen.findByRole("option", { name: "不使用协议包（透明转发）" }));

    expect(onChange).toHaveBeenCalledWith({ ...current, processing: { mode: "direct" } });
  });

  it("shows a loading state without exposing stale package options", async () => {
    const user = userEvent.setup();
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={{ ...catalog(), loading: true }}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("正在读取入口协议包目录")).toBeVisible();
    expect(screen.queryByText("当前处理方案已不可用")).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    expect(screen.queryByRole("option", { name: /ISO 8583/ })).not.toBeInTheDocument();
  });

  it("shows catalog errors and retries", async () => {
    const refresh = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={{ loading: false, error: "目录暂时不可用", refresh }}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("目录暂时不可用")).toBeVisible();
    expect(screen.queryByText("当前处理方案已不可用")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("explains an empty authoritative catalog", () => {
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={catalog([], { installed_version_count: 4, unavailable_version_count: 4 })}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("没有可绑定的 Socket 协议包版本")).toBeVisible();
    expect(screen.getByText(/已安装 4 个版本，其中 4 个当前不可用/)).toBeVisible();
  });

  it("lists only authoritative exact versions", async () => {
    const user = userEvent.setup();
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={catalog([packageOption("1.0.0"), packageOption("2.0.0")], {
          installed_version_count: 5,
          unavailable_version_count: 3,
        })}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    expect(await screen.findByRole("option", { name: "ISO 8583 1.0.0 · 1.0.0 · 外部 · 在线" })).toBeVisible();
    expect(screen.getByRole("option", { name: "ISO 8583 2.0.0 · 2.0.0 · 外部 · 在线" })).toBeVisible();
    expect(screen.getAllByRole("option")).toHaveLength(3);
  });

  it("does not expose HTTP packages in the Socket selector", async () => {
    const http = packageOption("2.0.0", { kind: "http", name: "HTTP JSON" });
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={settings()} catalog={catalog([packageOption(), http])} locked={false} onChange={vi.fn()} />);

    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    expect(await screen.findByRole("option", { name: "ISO 8583 1.0.0 · 1.0.0 · 外部 · 在线" })).toBeVisible();
    expect(screen.queryByRole("option", { name: /HTTP JSON/ })).not.toBeInTheDocument();
  });

  it("preserves a bound version that became unavailable", async () => {
    const user = userEvent.setup();
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={catalog([packageOption("2.0.0")], { installed_version_count: 2, unavailable_version_count: 1 })}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("当前处理方案已不可用")).toBeVisible();
    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    expect(await screen.findByRole("option", { name: "iso-8583@1.0.0 · 当前选择（不可用）" })).toHaveAttribute("aria-disabled", "true");
  });

  it("locks package changes while the entry is running", () => {
    render(<SocketProcessingCard settings={settings()} catalog={catalog()} locked onChange={vi.fn()} />);
    expect(screen.getByLabelText("Socket 协议处理方案")).toBeDisabled();
  });
});
