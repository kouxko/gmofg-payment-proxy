// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProtocolPackagesView } from "./protocol-packages-view";
import { deferred, detail, group, version } from "./protocol-packages-test-support";

const mocks = vi.hoisted(() => ({
  protocolPackageList: vi.fn(),
  protocolPackageDetail: vi.fn(),
  protocolPackageRestoreBuiltin: vi.fn(),
  protocolPackageExportBuiltin: vi.fn(),
  toast: vi.fn(),
  useAppEventRefresh: vi.fn(),
}));

vi.mock("@heroui/react", async (importOriginal) => ({
  ...await importOriginal<typeof import("@heroui/react")>(),
  toast: mocks.toast,
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    protocolPackageList: mocks.protocolPackageList,
    protocolPackageDetail: mocks.protocolPackageDetail,
    protocolPackageRestoreBuiltin: mocks.protocolPackageRestoreBuiltin,
    protocolPackageExportBuiltin: mocks.protocolPackageExportBuiltin,
  },
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : String(reason),
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: mocks.useAppEventRefresh,
}));

describe("ProtocolPackagesView list", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageList.mockResolvedValue([]);
    mocks.protocolPackageDetail.mockImplementation(async (packageRef) =>
      detail(version(packageRef.version)),
    );
    mocks.protocolPackageRestoreBuiltin.mockResolvedValue({
      outcome: "reused",
      version: version("1.0.0", {
        package: { id: "iso8583-ascii-standard", version: "1.0.0" },
        name: "ISO 8583 ASCII 示例",
        package_source: { type: "internal", built_in: true },
        enabled: true,
      }),
      capabilities: detail().capabilities,
      kind: detail().kind,
      upstream_schema: detail().upstream_schema,
      downstream_schema: detail().downstream_schema,
    });
    mocks.protocolPackageExportBuiltin.mockResolvedValue({
      path: "/tmp/iso8583-template.zip",
      bytes_written: 4096,
      replaced_existing: false,
    });
  });

  it("refreshes the catalog for authoritative package and snapshot events", () => {
    render(<ProtocolPackagesView />);

    // 列表与详情查询各自订阅同一权威目录事件；关闭中的详情也必须在下次打开前失效。
    expect(mocks.useAppEventRefresh).toHaveBeenCalledTimes(2);
    expect(mocks.useAppEventRefresh).toHaveBeenCalledWith(
      ["protocol_package_catalog_changed", "snapshot_required"],
      expect.any(Function),
    );
  });

  it("loads through protocolPackageList and renders loading then empty state", async () => {
    const pending = deferred<never[]>();
    mocks.protocolPackageList.mockReturnValue(pending.promise);
    render(<ProtocolPackagesView />);

    expect(screen.getByLabelText("正在读取协议包列表")).toBeVisible();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(1);
    pending.resolve([]);
    expect(await screen.findByText("尚未安装协议包")).toBeVisible();
    expect(screen.getByText("内置 ISO 8583:1987 ASCII Profile")).toBeVisible();
    expect(screen.getByText(/覆盖主位图、次位图和 DE2–DE128 字段结构/)).toBeVisible();
  });

  it("restores the built-in ISO example from the empty state, refreshes, and opens the exact version", async () => {
    const builtInVersion = version("1.0.0", {
      package: { id: "iso8583-ascii-standard", version: "1.0.0" },
      name: "ISO 8583 ASCII 示例",
      package_source: { type: "internal", built_in: true },
      enabled: true,
    });
    const builtInGroup = group({
      id: "iso8583-ascii-standard",
      name: "ISO 8583 ASCII 示例",
      versions: [builtInVersion],
      reference_count: 0,
      active_reference_count: 0,
    });
    mocks.protocolPackageList.mockResolvedValueOnce([]).mockResolvedValueOnce([builtInGroup]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(builtInVersion));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await screen.findByText("尚未安装协议包");
    await user.click(screen.getAllByRole("button", { name: "恢复 ISO 8583 示例包" }).at(-1)!);

    expect(mocks.protocolPackageRestoreBuiltin).toHaveBeenCalledTimes(1);
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(2);
    expect(await screen.findByRole("dialog", { name: "ISO 8583 ASCII 示例" })).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("官方 ISO 8583 示例已存在并通过重新校验。");
    expect(screen.getByText("内置示例", { selector: "dd" })).toBeVisible();
  });

  it("does not report success or refresh when the restore command fails", async () => {
    mocks.protocolPackageRestoreBuiltin.mockRejectedValue({
      code: "PROTOCOL_PACKAGE_BUILTIN_INVALID",
      message: "内置资产校验失败",
      field_errors: {},
      diagnostic: null,
    });
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await screen.findByText("尚未安装协议包");
    await user.click(screen.getAllByRole("button", { name: "恢复 ISO 8583 示例包" }).at(-1)!);

    expect(await screen.findByText("内置示例恢复失败")).toBeVisible();
    expect(screen.getByText("内置资产校验失败")).toBeVisible();
    expect(screen.queryByText(/已恢复并启用|已存在并通过/)).not.toBeInTheDocument();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "重试" }));
    await waitFor(() => expect(mocks.protocolPackageRestoreBuiltin).toHaveBeenCalledTimes(2));
  });

  it("fails closed for a mismatched restore response", async () => {
    mocks.protocolPackageRestoreBuiltin.mockResolvedValue({
      outcome: "installed",
      version: version("9.0.0", { package_source: { type: "internal", built_in: true } }),
    });
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await screen.findByText("尚未安装协议包");
    await user.click(screen.getAllByRole("button", { name: "恢复 ISO 8583 示例包" })[0]);

    expect(await screen.findByText("内置示例恢复结果不完整，请刷新列表后重试。")).toBeVisible();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(1);
  });

  it.each([
    [null, "内置示例已恢复，但刷新后的协议包列表数据不完整。"],
    [[], "内置示例已恢复，但列表中未找到官方精确版本。"],
  ])("reports an exact restore refresh failure for %j", async (refreshed, message) => {
    mocks.protocolPackageRestoreBuiltin.mockResolvedValue({
      outcome: "installed",
      version: version("1.0.0", {
        package: { id: "iso8583-ascii-standard", version: "1.0.0" },
        name: "ISO 8583 ASCII 示例",
        package_source: { type: "internal", built_in: true },
        enabled: true,
      }),
      capabilities: detail().capabilities,
      kind: detail().kind,
      upstream_schema: detail().upstream_schema,
      downstream_schema: detail().downstream_schema,
    });
    mocks.protocolPackageList.mockResolvedValueOnce([]).mockResolvedValueOnce(refreshed);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await screen.findByText("尚未安装协议包");
    await user.click(screen.getAllByRole("button", { name: "恢复 ISO 8583 示例包" }).at(-1)!);

    expect(await screen.findByText(message)).toBeVisible();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("dialog", { name: "ISO 8583 ASCII 示例" })).not.toBeInTheDocument();
  });

  it("shows the installed notice after restoring a missing built-in example", async () => {
    const restoredVersion = version("1.0.0", {
      package: { id: "iso8583-ascii-standard", version: "1.0.0" },
      name: "ISO 8583 ASCII 示例",
      package_source: { type: "internal", built_in: true },
      enabled: true,
    });
    const restoredGroup = group({
      id: "iso8583-ascii-standard",
      name: "ISO 8583 ASCII 示例",
      versions: [restoredVersion],
      reference_count: 0,
      active_reference_count: 0,
    });
    mocks.protocolPackageRestoreBuiltin.mockResolvedValue({
      outcome: "installed",
      version: restoredVersion,
      capabilities: detail().capabilities,
      kind: detail().kind,
      upstream_schema: detail().upstream_schema,
      downstream_schema: detail().downstream_schema,
    });
    mocks.protocolPackageList.mockResolvedValueOnce([]).mockResolvedValueOnce([restoredGroup]);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await screen.findByText("尚未安装协议包");
    await user.click(screen.getAllByRole("button", { name: "恢复 ISO 8583 示例包" }).at(-1)!);

    expect(await screen.findByRole("status")).toHaveTextContent("官方 ISO 8583 示例已恢复并启用。");
    expect(await screen.findByRole("dialog", { name: "ISO 8583 ASCII 示例" })).toBeVisible();
  });

  it("exports the built-in template once and reports the exact ZIP result", async () => {
    const exported = deferred<{ path: string; bytes_written: number; replaced_existing: boolean }>();
    mocks.protocolPackageExportBuiltin.mockReturnValue(exported.promise);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    const exportButton = screen.getAllByRole("button", { name: "导出 ISO 8583 模板 ZIP" })[0];

    await Promise.all([user.click(exportButton), user.click(exportButton)]);
    expect(mocks.protocolPackageExportBuiltin).toHaveBeenCalledTimes(1);
    expect(screen.getAllByRole("button", { name: "正在导出…" })[0]).toBeDisabled();

    exported.resolve({
      path: "/tmp/iso8583-template.zip",
      bytes_written: 8192,
      replaced_existing: true,
    });
    await waitFor(() => expect(mocks.toast).toHaveBeenCalledWith(
      "ISO 8583 模板 ZIP 已导出（8192 字节，已覆盖原文件）。",
      { variant: "success" },
    ));
    await waitFor(() => expect(screen.getAllByRole("button", { name: "导出 ISO 8583 模板 ZIP" })[0]).toBeEnabled());
  });

  it("reports a template export failure and restores the export action", async () => {
    mocks.protocolPackageExportBuiltin.mockRejectedValue(new Error("模板目录不可写"));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await user.click(screen.getAllByRole("button", { name: "导出 ISO 8583 模板 ZIP" }).at(-1)!);

    await waitFor(() => expect(mocks.toast).toHaveBeenCalledWith("模板目录不可写", { variant: "danger" }));
    expect(screen.getAllByRole("button", { name: "导出 ISO 8583 模板 ZIP" })[0]).toBeEnabled();
  });

  it("shows a recoverable list error", async () => {
    mocks.protocolPackageList.mockRejectedValueOnce(new Error("注册表暂不可用"));
    render(<ProtocolPackagesView />);

    expect(await screen.findByText("协议包列表读取失败")).toBeVisible();
    expect(screen.getByText("注册表暂不可用")).toBeVisible();
    await userEvent.setup().click(screen.getByRole("button", { name: "重试" }));
    await waitFor(() => expect(mocks.protocolPackageList).toHaveBeenCalledTimes(2));
  });

  it("renders status, version count, reference count and active usage", async () => {
    mocks.protocolPackageList.mockResolvedValue([group()]);
    render(<ProtocolPackagesView />);

    const row = await screen.findByRole("button", { name: "查看协议包 ISO 8583" });
    expect(row).toHaveClass("h-auto", "min-h-16");
    expect(row).toHaveTextContent("2.0.0");
    expect(row).toHaveTextContent("3 个版本");
    expect(row).toHaveTextContent("3 个引用");
    expect(row).toHaveTextContent("部分启用 1/3");
    expect(row).toHaveTextContent("1 个运行中");
  });

  it("shows HTTP and Socket packages in one list without protocol tabs", async () => {
    const httpVersion = version("1.0.0", {
      package: { id: "json-body", version: "1.0.0" },
      name: "JSON Body",
      kind: "http",
      enabled: true,
    });
    const httpGroup = group({
      id: "json-body",
      name: "JSON Body",
      kind: "http",
      versions: [httpVersion],
      reference_count: 0,
      active_reference_count: 0,
    });
    mocks.protocolPackageList.mockResolvedValue([group(), httpGroup]);
    render(<ProtocolPackagesView />);

    expect(await screen.findByRole("button", { name: "查看协议包 ISO 8583" })).toBeVisible();
    expect(screen.getByRole("button", { name: "查看协议包 JSON Body" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: "HTTP" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Socket" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "恢复 ISO 8583 示例包" })).toBeVisible();
    expect(screen.getByText("内置 ISO 8583:1987 ASCII Profile")).toBeVisible();
    expect(screen.getByRole("button", { name: "查看协议包 ISO 8583" })).toHaveTextContent("Socket");
    expect(screen.getByRole("button", { name: "查看协议包 JSON Body" })).toHaveTextContent("HTTP");
  });

  it("marks a built-in example in the package list", async () => {
    mocks.protocolPackageList.mockResolvedValue([group({
      versions: [version("1.0.0", { package_source: { type: "internal", built_in: true } })],
    })]);
    render(<ProtocolPackagesView />);

    expect(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }))
      .toHaveTextContent("内置示例");
  });

  it("opens by click, Enter and Space, then restores focus after closing", async () => {
    const user = userEvent.setup();
    mocks.protocolPackageList.mockResolvedValue([group()]);
    render(<ProtocolPackagesView />);
    const row = await screen.findByRole("button", { name: "查看协议包 ISO 8583" });

    await user.click(row);
    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "ISO 8583" })).not.toBeInTheDocument());
    await waitFor(() => expect(row).toHaveFocus());

    await user.keyboard("{Enter}");
    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "关闭协议包详情" }));
    await waitFor(() => expect(row).toHaveFocus());

    await user.keyboard(" ");
    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();
  });

  it("displays the Rust-ordered versions from newest to oldest", async () => {
    const user = userEvent.setup();
    mocks.protocolPackageList.mockResolvedValue([group()]);
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));

    const navigation = screen.getByRole("navigation", { name: "协议包版本" });
    const labels = within(navigation).getAllByRole("button").map((button) => button.textContent);
    expect(labels[0]).toContain("2.0.0");
    expect(labels[1]).toContain("1.10.0");
    expect(labels[2]).toContain("1.2.0");
  });

  it.each([
    ["空版本分组", [group({ versions: [] })]],
    [
      "跨包版本身份",
      [group({ versions: [version("2.0.0", { package: { id: "tlv", version: "2.0.0" } })] })],
    ],
    ["重复精确版本", [group({ versions: [version("2.0.0"), version("2.0.0")] })]],
    ["重复分组 ID", [group(), group({ name: "重复分组" })]],
  ])("fails closed for %s instead of opening a misleading row", async (_caseName, response) => {
    mocks.protocolPackageList.mockResolvedValue(response);
    render(<ProtocolPackagesView />);

    expect(await screen.findByText("协议包列表返回了不完整的数据。")).toBeVisible();
    expect(screen.queryByRole("button", { name: /查看协议包/ })).not.toBeInTheDocument();
    expect(mocks.protocolPackageDetail).not.toHaveBeenCalled();
  });

  it("renders disabled, mixed and invalid version badges without hiding errors", async () => {
    const user = userEvent.setup();
    mocks.protocolPackageList.mockResolvedValue([
      group({
        id: "disabled",
        name: "Disabled",
        versions: [version("1.0.0", {
          package: { id: "disabled", version: "1.0.0" },
          enabled: false,
        })],
        reference_count: 0,
        active_reference_count: 0,
      }),
      group({
        id: "invalid",
        name: "Invalid",
        versions: [
          version("2.0.0", { package: { id: "invalid", version: "2.0.0" } }),
          version("3.0.0", {
            package: { id: "invalid", version: "3.0.0" },
            enabled: false,
            validation: { state: "invalid", code: "COMPILE_FAILED" },
          }),
        ],
        reference_count: 0,
        active_reference_count: 0,
      }),
    ]);
    render(<ProtocolPackagesView />);

    expect(await screen.findByRole("button", { name: "查看协议包 Disabled" })).toHaveTextContent("已停用");
    const invalidRow = screen.getByRole("button", { name: "查看协议包 Invalid" });
    expect(invalidRow).toHaveTextContent("已启用");
    expect(invalidRow).toHaveTextContent("1 个校验异常");
    await user.click(invalidRow);
    expect(await screen.findByRole("button", { name: /3\.0\.0校验失败：COMPILE_FAILED/ })).toHaveTextContent("无效");
  });

  it("fails closed when required list counters are missing", async () => {
    mocks.protocolPackageList.mockResolvedValue([
      group({ reference_count: undefined as never }),
    ]);
    render(<ProtocolPackagesView />);

    expect(await screen.findByText("协议包列表返回了不完整的数据。")).toBeVisible();
    expect(screen.queryByRole("button", { name: /查看协议包/ })).not.toBeInTheDocument();
  });

  it("shows external source and online state in both list and detail", async () => {
    const external = version("4.0.0", {
      package: { id: "vendor-iso", version: "4.0.0" },
      name: "Vendor ISO",
      package_source: { type: "external", online: true },
      enabled: true,
    });
    mocks.protocolPackageList.mockResolvedValue([group({
      id: "vendor-iso",
      name: "Vendor ISO",
      versions: [external],
      reference_count: 0,
      active_reference_count: 0,
    })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(external));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    const row = await screen.findByRole("button", { name: "查看协议包 Vendor ISO" });
    expect(row).toHaveTextContent("外部软件包");
    expect(row).toHaveTextContent("外部在线");
    await user.click(row);
    expect(await screen.findByText("外部 · 在线")).toBeVisible();
  });
});
