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
  protocolPackageEnable: vi.fn(),
  protocolPackageDisable: vi.fn(),
  protocolPackageRestart: vi.fn(),
  protocolPackageDelete: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    protocolPackageList: mocks.protocolPackageList,
    protocolPackageDetail: mocks.protocolPackageDetail,
    protocolPackageEnable: mocks.protocolPackageEnable,
    protocolPackageDisable: mocks.protocolPackageDisable,
    protocolPackageRestart: mocks.protocolPackageRestart,
    protocolPackageDelete: mocks.protocolPackageDelete,
  },
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : String(reason),
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

describe("ProtocolPackageDialog details", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageList.mockResolvedValue([group()]);
    mocks.protocolPackageEnable.mockImplementation(async (packageRef) => version(packageRef.version, {
      package: packageRef,
      enabled: true,
    }));
    mocks.protocolPackageDisable.mockImplementation(async (packageRef) => version(packageRef.version, {
      package: packageRef,
      package_source: { type: "external", online: true },
      enabled: false,
    }));
    mocks.protocolPackageRestart.mockImplementation(async (packageRef) => version(packageRef.version, {
      package: packageRef,
      package_source: { type: "external", online: true },
      enabled: true,
    }));
    mocks.protocolPackageDelete.mockResolvedValue({
      success: true,
      cancelled: false,
      message: "协议包版本已删除。",
      ui_tone: "positive",
      entity_id: "iso-8583@2.0.0",
      revision: null,
      requires_restart: false,
    });
    mocks.protocolPackageDetail.mockImplementation(async (packageRef) => {
      const selected = version(packageRef.version, {
        name: packageRef.version === "1.10.0" ? "旧版专用名称" : "最新版专用名称",
      });
      return detail(selected, {
        upstream_schema: {
          root: { type: "object", title: packageRef.version === "1.10.0" ? "旧版 Schema" : "最新版 Schema", properties: packageRef.version === "1.10.0"
            ? { old_field: { type: "boolean", title: "旧版字段" } }
            : { new_field: { type: "array", title: "新版字段", items: { type: "number" } } } },
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
    expect(within(table).getByText("/new_field")).toBeVisible();
    expect(within(table).getByText("新版字段")).toBeVisible();
    expect(within(table).getByText("array")).toBeVisible();
  });

  it("shows the external connection contract without exposing payloads", async () => {
    mocks.protocolPackageList.mockResolvedValue([group({
      versions: [version("2.0.0", { package_source: { type: "external", online: true } })],
    })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(
      version("2.0.0", { package_source: { type: "external", online: true } }),
      {
        external: {
          local_process: false,
          remote_address: "127.0.0.1:49152",
          connection_id: "018f6fc0-65d8-7d90-b25b-392f6d9b9481",
          first_connected_at: "2026-08-20T08:00:00Z",
          last_connected_at: "2026-08-20T09:00:00Z",
          registration_fingerprint_sha256: "ab".repeat(32),
          upstream_methods: {
            frame: "hooks.upstream.split",
            decode: "hooks.upstream.decode",
            encode: "hooks.upstream.encode",
            display: "document.upstream.render",
          },
          downstream_methods: {
            frame: "hooks.downstream.split",
            decode: "hooks.downstream.decode",
            encode: "hooks.downstream.encode",
            display: "document.downstream.render",
          },
          recent_error: {
            code: "EXTERNAL_PACKAGE_DISCONNECTED",
            message: "外部软件包连接已断开。",
            occurred_at: "2026-08-20T09:01:00Z",
          },
        },
      },
    ));

    await openDialog();
    expect(screen.getByText("127.0.0.1:49152")).toBeVisible();
    expect(screen.getByText(/hooks\.upstream\.split/)).toBeVisible();
    expect(screen.getByText(/最近错误：EXTERNAL_PACKAGE_DISCONNECTED/)).toBeVisible();
    expect(screen.queryByText(/payload/i)).not.toBeInTheDocument();
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
      id: "iso8583-ascii-standard",
      versions: [version("1.0.0", {
        package: { id: "iso8583-ascii-standard", version: "1.0.0" },
        package_source: { type: "external", online: true },
      })],
    })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("1.0.0", {
      package: { id: "iso8583-ascii-standard", version: "1.0.0" },
      package_source: { type: "external", online: true },
    })));
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
    expect(screen.getByText("/old_field")).toBeVisible();
    expect(screen.queryByText("/new_field")).not.toBeInTheDocument();
    expect(mocks.protocolPackageDetail).toHaveBeenLastCalledWith({ id: "iso-8583", version: "1.10.0" });
  });

  it("ignores an older deferred response after switching versions", async () => {
    const latest = deferred<ReturnType<typeof detail>>();
    mocks.protocolPackageDetail.mockImplementation(async (packageRef) => {
      if (packageRef.version === "2.0.0") return latest.promise;
      return detail(version("1.10.0", { name: "当前旧版" }), {
        upstream_schema: {
          root: { type: "object", title: "当前 Schema", properties: { current_field: { type: "string", title: "当前字段" } } },
        },
      });
    });
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(screen.getByRole("button", { name: /1\.10\.0/ }));
    expect(await screen.findByText("/current_field")).toBeVisible();

    latest.resolve(detail(version("2.0.0"), {
      upstream_schema: {
        root: { type: "object", title: "迟到 Schema", properties: { late_field: { type: "string", title: "迟到字段" } } },
      },
    }));

    await waitFor(() => expect(screen.getByText("/current_field")).toBeVisible());
    expect(screen.queryByText("/late_field")).not.toBeInTheDocument();
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
      upstream_schema: { root: { type: "object", title: "长字段 Schema", properties: { long_field: { type: "string", title: longName } } } },
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
    expect(screen.queryByText(/protocol\.js|manifest\.toml|源码内容/)).not.toBeInTheDocument();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(1);
    expect(mocks.protocolPackageDetail).toHaveBeenCalledTimes(1);
    expect(Object.keys({ protocolPackageList: mocks.protocolPackageList, protocolPackageDetail: mocks.protocolPackageDetail }))
      .toEqual(["protocolPackageList", "protocolPackageDetail"]);
  });

  it("rejects a malformed recursive Schema instead of presenting impossible Rust data", async () => {
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("2.0.0"), {
      upstream_schema: { root: { type: "array" } as never },
      downstream_schema: { root: { type: "object", title: "空响应 Schema", properties: {} } },
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

  it("enables an imported version and updates its availability without a reload", async () => {
    const disabled = version("1.0.0", { enabled: false, name: "用户导入协议包" });
    mocks.protocolPackageList.mockResolvedValue([group({
      versions: [disabled],
      reference_count: 0,
      active_reference_count: 0,
    })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(disabled, { usages: [] }));
    mocks.protocolPackageEnable.mockResolvedValue({ ...disabled, enabled: true });

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(await screen.findByRole("button", { name: "启用协议包" }));

    await waitFor(() => expect(mocks.protocolPackageEnable).toHaveBeenCalledWith(disabled.package));
    expect(await screen.findByText("已启用", { selector: "dd" })).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("已启用，可在入口配置中选择");
    expect(screen.queryByRole("button", { name: "启用协议包" })).not.toBeInTheDocument();
  });

  it("keeps a version disabled and restores the enable action after failure", async () => {
    const disabled = version("1.0.0", { enabled: false, name: "用户导入协议包" });
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [disabled] })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(disabled));
    mocks.protocolPackageEnable.mockRejectedValue(new Error("协议脚本编译失败"));

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(await screen.findByRole("button", { name: "启用协议包" }));

    expect(await screen.findByText("协议包启用失败")).toBeVisible();
    expect(screen.getByText("协议脚本编译失败")).toBeVisible();
    expect(screen.getByText("已停用", { selector: "dd" })).toBeVisible();
    expect(screen.getByRole("button", { name: "启用协议包" })).toBeEnabled();
  });

  it("disables an external package once and updates the exact version after success", async () => {
    const external = version("2.0.0", {
      package_source: { type: "external", online: true },
      enabled: true,
    });
    const pending = deferred<ReturnType<typeof version>>();
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [external] })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(external, { usages: [] }));
    mocks.protocolPackageDisable.mockReturnValue(pending.promise);

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    const disableButton = await screen.findByRole("button", { name: "停用协议包" });
    await Promise.all([user.click(disableButton), user.click(disableButton)]);

    expect(mocks.protocolPackageDisable).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "正在停用…" })).toBeDisabled();
    pending.resolve({ ...external, enabled: false });
    expect(await screen.findByText("已停用", { selector: "dd" })).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("已停用");
    expect(screen.queryByRole("button", { name: "停用协议包" })).not.toBeInTheDocument();
  });

  it("keeps the external package enabled and exposes a retry after disable fails", async () => {
    const external = version("2.0.0", {
      package_source: { type: "external", online: true },
      enabled: true,
    });
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [external] })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(external, { usages: [] }));
    mocks.protocolPackageDisable.mockRejectedValue(new Error("入口停止失败"));

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(await screen.findByRole("button", { name: "停用协议包" }));

    expect(await screen.findByText("协议包停用失败")).toBeVisible();
    expect(screen.getByText("入口停止失败")).toBeVisible();
    expect(screen.getByText("已启用", { selector: "dd" })).toBeVisible();
    expect(screen.getByRole("button", { name: "停用协议包" })).toBeEnabled();
  });

  it("blocks deletion from the authoritative usage list and explains every reference", async () => {
    const external = version("2.0.0", {
      package_source: { type: "external", online: false },
      enabled: false,
    });
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [external] })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(external));

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));

    expect(await screen.findByText("仍有 1 个入口引用此精确版本，不能删除。")).toBeVisible();
    expect(screen.getByText("请先修改或删除：收银台测试 / 上游 Socket")).toBeVisible();
    expect(screen.getByRole("button", { name: "删除协议包" })).toBeDisabled();
    expect(mocks.protocolPackageDelete).not.toHaveBeenCalled();
  });

  it("confirms deletion, keeps the confirmation locked while pending, then refreshes and restores focus", async () => {
    const external = version("2.0.0", {
      package_source: { type: "external", online: true },
      enabled: false,
    });
    const pending = deferred<{
      success: boolean;
      cancelled: boolean;
      message: string;
      ui_tone: "positive";
      entity_id: string;
      revision: null;
      requires_restart: boolean;
    }>();
    mocks.protocolPackageList.mockResolvedValueOnce([group({ versions: [external] })]).mockResolvedValueOnce([]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(external, { usages: [] }));
    mocks.protocolPackageDelete.mockReturnValue(pending.promise);

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(await screen.findByRole("button", { name: "删除协议包" }));
    expect(screen.getByRole("alertdialog", { name: "删除 ISO 8583 长名称协议包 2.0.0？" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认删除" }));

    expect(screen.getByRole("button", { name: "正在删除…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    pending.resolve({
      success: true,
      cancelled: false,
      message: "协议包版本已删除。",
      ui_tone: "positive",
      entity_id: "iso-8583@2.0.0",
      revision: null,
      requires_restart: false,
    });

    await waitFor(() => expect(mocks.protocolPackageList).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("尚未安装协议包")).toBeVisible();
    await waitFor(() => expect(screen.getByRole("heading", { name: "协议包" })).toHaveFocus());
  });

  it("keeps the delete confirmation open and reports a backend rejection", async () => {
    const external = version("2.0.0", {
      package_source: { type: "external", online: false },
      enabled: false,
    });
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [external] })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(external, { usages: [] }));
    mocks.protocolPackageDelete.mockRejectedValue(new Error("注册状态已变化，请刷新"));

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    await user.click(await screen.findByRole("button", { name: "删除协议包" }));
    await user.click(screen.getByRole("button", { name: "确认删除" }));

    expect(await screen.findByText("协议包删除失败")).toBeVisible();
    expect(screen.getByText("注册状态已变化，请刷新")).toBeVisible();
    expect(screen.getByRole("button", { name: "确认删除" })).toBeEnabled();
    expect(screen.getByRole("alertdialog")).toBeVisible();
  });
  it("exposes the unified lifecycle controls for every package", async () => {
    await openDialog();
    expect(screen.getByRole("button", { name: "停用协议包" })).toBeVisible();
    expect(screen.getByRole("button", { name: "删除协议包" })).toBeVisible();
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
