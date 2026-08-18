import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
  SocketRelaySettings,
} from "@/generated/rust-types";
import { SocketProcessingCard, type ProtocolCatalogState } from "./socket-processing-card";

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
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: true },
      display: true,
    },
    schema: {
      id: "iso-message",
      version: 3,
      title: "ISO Message",
      fields: [{ name: "mti", label: "MTI", type: "string" }],
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
    processing: {
      mode: "scripted",
      settings: {
        package: { id: "iso-8583", version: "1.0.0" },
        upstream: { decode_enabled: false, encode_enabled: false },
        downstream: { decode_enabled: false, encode_enabled: false },
      },
    },
  };
}

describe("SocketProcessingCard", () => {
  it("does not render protocol controls for raw processing", () => {
    const direct = settings();
    direct.processing = { mode: "direct" };

    const { container } = render(
      <SocketProcessingCard settings={direct} catalog={catalog()} locked={false} onChange={vi.fn()} />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("renders four result-oriented relay controls with technical identity in advanced details", async () => {
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={settings()} catalog={catalog()} locked={false} onChange={vi.fn()} />);

    expect(screen.getByRole("switch", { name: "读取应用发往远端的内容" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "按规则改写应用发往远端的内容" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "读取远端返回应用的内容" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "按规则改写远端返回应用的内容" })).toBeEnabled();
    expect(screen.getByText("高级技术信息").closest("details")).not.toHaveAttribute("open");
    expect(screen.getByText("iso-8583@1.0.0", { selector: "span" })).not.toBeVisible();
    expect(screen.getByText("Schema iso-message v3")).not.toBeVisible();
    await user.click(screen.getByText("高级技术信息"));
    expect(screen.getByText("iso-8583@1.0.0", { selector: "span" })).toBeVisible();
    expect(screen.getByText("Schema iso-message v3")).toBeVisible();
  });

  it("renders only the two meaningful LocalResponder controls without Server, upstream, DNS or test controls", () => {
    render(
      <SocketProcessingCard settings={settings("local_responder")} catalog={catalog()} locked={false} onChange={vi.fn()} />,
    );

    expect(screen.getByRole("switch", { name: "读取应用请求内容" })).toBeEnabled();
    expect(screen.getByRole("switch", { name: "按规则生成应答内容" })).toBeEnabled();
    expect(screen.queryByRole("switch", { name: /Server|upstream/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /DNS|连接测试|上游测试/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/Server TLS|上游主机|上游端口/)).not.toBeInTheDocument();
  });

  it("submits an exact local-response payload when request parsing changes", async () => {
    const onChange = vi.fn();
    const current = settings("local_responder");
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={current} catalog={catalog()} locked={false} onChange={onChange} />);

    await user.click(screen.getByRole("switch", { name: "读取应用请求内容" }));

    expect(onChange).toHaveBeenCalledWith({
      ...current,
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: true, encode_enabled: false },
          downstream: { decode_enabled: false, encode_enabled: false },
        },
      },
    });
  });

  it("submits an exact local-response payload when response generation changes", async () => {
    const onChange = vi.fn();
    const current = settings("local_responder");
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={current} catalog={catalog()} locked={false} onChange={onChange} />);

    await user.click(screen.getByRole("switch", { name: "按规则生成应答内容" }));

    expect(onChange).toHaveBeenCalledWith({
      ...current,
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: false, encode_enabled: false },
          downstream: { decode_enabled: false, encode_enabled: true },
        },
      },
    });
  });

  it("updates only relay response parsing for the selected direction", async () => {
    const onChange = vi.fn();
    const current = settings();
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={current} catalog={catalog()} locked={false} onChange={onChange} />);

    await user.click(screen.getByRole("switch", { name: "读取远端返回应用的内容" }));

    expect(onChange).toHaveBeenCalledWith({
      ...current,
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: false, encode_enabled: false },
          downstream: { decode_enabled: true, encode_enabled: false },
        },
      },
    });
  });

  it("updates only relay request generation for the selected direction", async () => {
    const onChange = vi.fn();
    const current = settings();
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={current} catalog={catalog()} locked={false} onChange={onChange} />);

    await user.click(screen.getByRole("switch", { name: "按规则改写应用发往远端的内容" }));

    expect(onChange).toHaveBeenCalledWith({
      ...current,
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "1.0.0" },
          upstream: { decode_enabled: false, encode_enabled: true },
          downstream: { decode_enabled: false, encode_enabled: false },
        },
      },
    });
  });

  it("shows a loading state and disables package selection while the catalog is loading", () => {
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={{ loading: true, refresh: vi.fn(), data: undefined }}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("正在读取 Listener 协议包目录")).toBeVisible();
    expect(screen.getByLabelText("Socket 协议处理方案")).toBeDisabled();
    expect(screen.getAllByRole("switch").every((control) => control.hasAttribute("disabled")))
      .toBe(true);
  });

  it("shows catalog errors and retries through the supplied refresh action", async () => {
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
    expect(screen.getByLabelText("Socket 协议处理方案")).toBeDisabled();
    expect(screen.getAllByRole("switch").every((control) => control.hasAttribute("disabled")))
      .toBe(true);
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("explains an empty filtered catalog with installed and unavailable counts", () => {
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={catalog([], { installed_version_count: 4, unavailable_version_count: 4 })}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("没有可绑定的协议包版本")).toBeVisible();
    expect(screen.getByText(/已安装 4 个版本，其中 4 个当前不可用/)).toBeVisible();
  });

  it("lists multiple exact versions while excluding unavailable candidates from the authoritative catalog", async () => {
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
    expect(await screen.findByRole("option", { name: "ISO 8583 1.0.0 · 1.0.0" })).toBeVisible();
    expect(screen.getByRole("option", { name: "ISO 8583 2.0.0 · 2.0.0" })).toBeVisible();
    expect(screen.getAllByRole("option")).toHaveLength(2);
  });

  it("preserves and labels a currently bound exact version that became unavailable", async () => {
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
    expect(screen.getByText(/原选择仍会保留/)).toBeVisible();
    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    expect(await screen.findByRole("option", { name: "当前选择（不可用）" }))
      .toHaveAttribute("aria-disabled", "true");
  });

  it("atomically turns off unsupported generation and announces the affected direction", async () => {
    const current = settings();
    if (current.processing?.mode !== "scripted") throw new Error("expected scripted settings");
    current.processing.settings.downstream.encode_enabled = true;
    const unsupported = packageOption("2.0.0", {
      capabilities: {
        upstream: { frame: true, decode: true, encode: true },
        downstream: { frame: true, decode: true, encode: false },
        display: false,
      },
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<SocketProcessingCard settings={current} catalog={catalog([packageOption(), unsupported])} locked={false} onChange={onChange} />);

    await user.click(screen.getByLabelText("Socket 协议处理方案"));
    await user.click(await screen.findByRole("option", { name: "ISO 8583 2.0.0 · 2.0.0" }));

    expect(onChange).toHaveBeenCalledWith({
      ...current,
      processing: {
        mode: "scripted",
        settings: {
          package: { id: "iso-8583", version: "2.0.0" },
          upstream: { decode_enabled: false, encode_enabled: false },
          downstream: { decode_enabled: false, encode_enabled: false },
        },
      },
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "已选择 ISO 8583 2.0.0；因处理方案限制已关闭：改写远端返回应用的内容",
    );
    expect(screen.queryByRole("switch", { name: /Display/i })).not.toBeInTheDocument();
  });

  it("locks package and direction controls for running or unknown persisted state", () => {
    render(<SocketProcessingCard settings={settings()} catalog={catalog()} locked onChange={vi.fn()} />);

    expect(screen.getByLabelText("Socket 协议处理方案")).toBeDisabled();
    for (const control of screen.getAllByRole("switch")) expect(control).toBeDisabled();
  });

  it("keeps an unsaved new Listener editable when no runtime snapshot exists", () => {
    render(<SocketProcessingCard settings={settings()} catalog={catalog()} locked={false} onChange={vi.fn()} />);

    expect(screen.getByLabelText("Socket 协议处理方案")).toBeEnabled();
    for (const control of screen.getAllByRole("switch")) expect(control).toBeEnabled();
  });

  it("disables local response generation when the bound package lacks that capability", () => {
    const unavailableEncode = packageOption("1.0.0", {
      capabilities: {
        upstream: { frame: true, decode: true, encode: true },
        downstream: { frame: true, decode: true, encode: false },
        display: false,
      },
    });
    render(
      <SocketProcessingCard
        settings={settings("local_responder")}
        catalog={catalog([unavailableEncode])}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("switch", { name: "按规则生成应答内容" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "读取应用请求内容" })).toBeEnabled();
  });

  it.each([
    ["loading", { loading: true, refresh: vi.fn(), data: undefined }],
    ["error", { loading: false, error: "catalog failed", refresh: vi.fn(), data: undefined }],
    ["empty", catalog([], { installed_version_count: 0, unavailable_version_count: 0 })],
    ["bound version unavailable", catalog([packageOption("2.0.0")], { installed_version_count: 2, unavailable_version_count: 1 })],
  ] as const)("fails closed for all processing switches while the catalog is %s", (_state, catalogState) => {
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={catalogState}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    for (const control of screen.getAllByRole("switch")) expect(control).toBeDisabled();
  });

  it.each([
    ["refreshing", { ...catalog(), loading: true }],
    ["failed refresh", { ...catalog(), error: "fresh preflight failed" }],
  ] as const)("does not reuse stale catalog data while %s", (_state, catalogState) => {
    render(
      <SocketProcessingCard
        settings={settings()}
        catalog={catalogState}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    for (const control of screen.getAllByRole("switch")) expect(control).toBeDisabled();
    expect(screen.queryByText("当前处理方案已不可用")).not.toBeInTheDocument();
  });
});
