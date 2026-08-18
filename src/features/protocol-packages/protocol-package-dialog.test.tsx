// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProtocolPackagesView } from "./protocol-packages-view";
import {
  deferred,
  detail,
  group,
  version,
} from "./protocol-packages-test-support";

const mocks = vi.hoisted(() => ({
  protocolPackageList: vi.fn(),
  protocolPackageDetail: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    protocolPackageList: mocks.protocolPackageList,
    protocolPackageDetail: mocks.protocolPackageDetail,
  },
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : String(reason),
}));

describe("ProtocolPackageDialog details", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageList.mockResolvedValue([group()]);
    mocks.protocolPackageDetail.mockImplementation(async (packageRef) => {
      const selected = version(packageRef.version, {
        name: packageRef.version === "1.10.0" ? "旧版专用名称" : "最新版专用名称",
      });
      const schemaVersion = packageRef.version.replaceAll(".", "-");
      return detail(selected, {
        upstream_schema: {
          id: `schema-${schemaVersion}`,
          version: packageRef.version === "1.10.0" ? 7 : 8,
          title: packageRef.version === "1.10.0" ? "旧版 Schema" : "最新版 Schema",
          fields: packageRef.version === "1.10.0"
            ? [{ name: "old_field", label: "旧版字段", type: "bool" }]
            : [{ name: "new_field", label: "新版字段", type: "blob" }],
        },
      });
    });
  });

  async function openDialog() {
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await screen.findByText(/最新版专用名称|ISO 8583 长名称协议包/);
    return user;
  }

  it("shows identity, capabilities, validation, usages and Schema columns", async () => {
    await openDialog();
    expect(screen.getByText("iso-8583", { selector: "dd" })).toBeVisible();
    expect(screen.getByText("Host API")).toBeVisible();
    expect(screen.getByText("校验通过")).toBeVisible();
    expect(screen.getByText("上行 Encode：支持")).toBeVisible();
    expect(screen.getByText("下行 Encode：支持")).toBeVisible();
    expect(screen.getByText("收银台测试 / 上游 Socket")).toBeVisible();
    expect(screen.getByText("workspace-1 / listener-1")).toBeVisible();
    expect(screen.getByText("已启用 · 运行中")).toBeVisible();
    expect(screen.getByText("适用协议")).toBeVisible();
    expect(screen.getByText("Socket", { selector: "dd" })).toBeVisible();
    const table = screen.getByRole("grid", { name: "上行 Schema 字段" });
    expect(within(table).getByText("字段名")).toBeVisible();
    expect(within(table).getByText("标签")).toBeVisible();
    expect(within(table).getByText("类型")).toBeVisible();
    expect(within(table).getByText("new_field")).toBeVisible();
    expect(within(table).getByText("新版字段")).toBeVisible();
    expect(within(table).getByText("blob")).toBeVisible();
  });

  it("shows HTTP as unframed while retaining request and response decoding", async () => {
    mocks.protocolPackageList.mockResolvedValue([group({
      kind: "http",
      versions: [version("2.0.0", { kind: "http" })],
    })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("2.0.0", { kind: "http" }), {
      kind: "http",
      capabilities: {
        upstream: { frame: false, decode: true, encode: true },
        downstream: { frame: false, decode: true, encode: true },
        display: true,
      },
    }));

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("tab", { name: "HTTP" }));
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await screen.findByText("ISO 8583 长名称协议包");
    expect(screen.getByText("HTTP", { selector: "dd" })).toBeVisible();
    expect(screen.getByText("上行 Frame：不支持")).toBeVisible();
    expect(screen.getByText("下行 Frame：不支持")).toBeVisible();
    expect(screen.getByRole("grid", { name: "请求 Schema 字段" })).toBeVisible();
    expect(screen.getByRole("grid", { name: "响应 Schema 字段" })).toBeVisible();
  });

  it("explains the exact scope and limitations of the built-in ISO example", async () => {
    mocks.protocolPackageList.mockResolvedValue([group({
      versions: [version("1.0.0", { built_in: true })],
    })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("1.0.0", { built_in: true })));
    await openDialog();

    const dialog = screen.getByRole("dialog", { name: "ISO 8583" });
    expect(within(dialog).getByText("ISO 8583:1987 ASCII Profile")).toBeVisible();
    expect(within(dialog).getByText(/覆盖主位图、次位图和 DE2–DE128 字段结构/)).toBeVisible();
    expect(within(dialog).getByText(/仍需按对端的字段编码和私有域规格调整/)).toBeVisible();
  });

  it("requests exact selected versions and never mixes their details", async () => {
    const user = await openDialog();
    expect(mocks.protocolPackageDetail).toHaveBeenLastCalledWith({ id: "iso-8583", version: "2.0.0" });

    await user.click(screen.getByRole("button", { name: /1\.10\.0/ }));
    expect(await screen.findByText("旧版专用名称")).toBeVisible();
    expect(screen.getByText("old_field")).toBeVisible();
    expect(screen.queryByText("new_field")).not.toBeInTheDocument();
    expect(mocks.protocolPackageDetail).toHaveBeenLastCalledWith({ id: "iso-8583", version: "1.10.0" });
  });

  it("ignores an older deferred response after switching versions", async () => {
    const latest = deferred<ReturnType<typeof detail>>();
    mocks.protocolPackageDetail.mockImplementation(async (packageRef) => {
      if (packageRef.version === "2.0.0") return latest.promise;
      return detail(version("1.10.0", { name: "当前旧版" }), {
        upstream_schema: {
          id: "old-schema",
          version: 1,
          title: "当前 Schema",
          fields: [{ name: "current_field", label: "当前字段", type: "string" }],
        },
      });
    });
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(screen.getByRole("button", { name: /1\.10\.0/ }));
    expect(await screen.findByText("current_field")).toBeVisible();

    latest.resolve(detail(version("2.0.0"), {
      upstream_schema: {
        id: "late-schema",
        version: 1,
        title: "迟到 Schema",
        fields: [{ name: "late_field", label: "迟到字段", type: "string" }],
      },
    }));

    await waitFor(() => expect(screen.getByText("current_field")).toBeVisible());
    expect(screen.queryByText("late_field")).not.toBeInTheDocument();
  });

  it("fails closed when detail identity does not match the selected version", async () => {
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("1.10.0")));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));

    expect(await screen.findByText("协议包详情身份与当前选择不一致。")).toBeVisible();
    expect(screen.queryByRole("grid", { name: "上行 Schema 字段" })).not.toBeInTheDocument();
  });

  it("defends long content and valid empty usage collections", async () => {
    const longName = `field_${"x".repeat(100)}`;
    mocks.protocolPackageList.mockResolvedValue([
      group({ name: `协议包${"很长".repeat(80)}`, versions: [version("3.0.0")] }),
    ]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("3.0.0"), {
      upstream_schema: { id: "long-schema", version: 1, title: "长字段 Schema", fields: [{ name: "long_field", label: longName, type: "string" }] },
      usages: [],
    }));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: /查看协议包/ }));

    expect(await screen.findByText(longName)).toBeVisible();
    expect(screen.getByText("当前没有入口引用此版本。")).toBeVisible();
  });

  it("contains no source entry, source text, or source request", async () => {
    await openDialog();
    expect(screen.queryByRole("button", { name: /源码|源代码|Source/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/protocol\.rhai|manifest\.toml|源码内容/)).not.toBeInTheDocument();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(1);
    expect(mocks.protocolPackageDetail).toHaveBeenCalledTimes(1);
    expect(Object.keys({ protocolPackageList: mocks.protocolPackageList, protocolPackageDetail: mocks.protocolPackageDetail }))
      .toEqual(["protocolPackageList", "protocolPackageDetail"]);
  });

  it("rejects an empty Schema instead of presenting impossible Rust data", async () => {
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("2.0.0"), {
      upstream_schema: { id: "empty", version: 1, title: "空 Schema", fields: [] },
      downstream_schema: { id: "empty-response", version: 1, title: "空响应 Schema", fields: [] },
      usages: [],
    }));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    expect(await screen.findByText("协议包详情数据不完整。")).toBeVisible();
    expect(screen.queryByRole("grid", { name: "上行 Schema 字段" })).not.toBeInTheDocument();
  });

  it("renders detail request errors without stale content", async () => {
    mocks.protocolPackageDetail.mockRejectedValue(new Error("版本详情不可用"));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    expect(await screen.findByText("协议包详情读取失败")).toBeVisible();
    expect(screen.getByText("版本详情不可用")).toBeVisible();
    expect(screen.queryByText("new_field")).not.toBeInTheDocument();
  });

  it("fails closed instead of masking missing required detail collections", async () => {
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("2.0.0"), {
      downstream_schema: undefined as never,
    }));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));

    expect(await screen.findByText("协议包详情数据不完整。")).toBeVisible();
    expect(screen.queryByText("此 Schema 没有声明字段。")).not.toBeInTheDocument();
  });

  it("fails closed when a completed detail request returns null", async () => {
    mocks.protocolPackageDetail.mockResolvedValue(null);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));

    expect(await screen.findByText("协议包详情数据不完整。")).toBeVisible();
    expect(screen.queryByText("选择一个版本查看详情。")).not.toBeInTheDocument();
  });

  it("keeps narrow dialogs scrollable with keyboard-accessible version actions", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 480 });
    window.dispatchEvent(new Event("resize"));
    await openDialog();

    const dialog = screen.getByRole("dialog", { name: "ISO 8583" });
    expect(dialog.querySelector(".overflow-y-auto")).not.toBeNull();
    const versionNavigation = screen.getByRole("navigation", { name: "协议包版本" });
    expect(versionNavigation.querySelector(".overflow-x-auto")).not.toBeNull();
    expect(within(versionNavigation).getByRole("button", { name: /1\.10\.0/ })).toBeEnabled();
  });
});
