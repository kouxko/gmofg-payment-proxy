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

describe("ProtocolPackagesView list", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageList.mockResolvedValue([]);
    mocks.protocolPackageDetail.mockImplementation(async (packageRef) =>
      detail(version(packageRef.version)),
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
});
