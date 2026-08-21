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
  importPreview,
  importResult,
  version,
} from "./protocol-packages-test-support";

const mocks = vi.hoisted(() => ({
  protocolPackageList: vi.fn(),
  protocolPackageDetail: vi.fn(),
  protocolPackageImport: vi.fn(),
  protocolPackageImportCommit: vi.fn(),
  protocolPackageImportDiscard: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    protocolPackageList: mocks.protocolPackageList,
    protocolPackageDetail: mocks.protocolPackageDetail,
    protocolPackageImport: mocks.protocolPackageImport,
    protocolPackageImportCommit: mocks.protocolPackageImportCommit,
    protocolPackageImportDiscard: mocks.protocolPackageImportDiscard,
  },
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : String(reason),
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

function importedGroup() {
  return group({
    versions: [
      version("1.2.0"),
      version("2.0.0"),
      version("3.0.0", {
        package: { id: "iso-8583", version: "3.0.0" },
        name: "ISO 8583 导入包",
        enabled: false,
      }),
    ],
  });
}

function appError(code: string, message: string, detailText: string) {
  return {
    code,
    message,
    field_errors: { archive: [detailText] },
    retryable: false,
    suggested_action: null,
    entity_id: null,
    runtime_epoch: null,
  };
}

describe("ProtocolPackage ZIP import", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageList.mockResolvedValue([]);
    mocks.protocolPackageImport.mockResolvedValue(importPreview());
    mocks.protocolPackageImportCommit.mockResolvedValue(importResult());
    mocks.protocolPackageImportDiscard.mockResolvedValue({ success: true, message: "已释放" });
    mocks.protocolPackageDetail.mockResolvedValue(detail(version("3.0.0", {
      package: { id: "iso-8583", version: "3.0.0" },
      name: "ISO 8583 导入包",
    })));
  });

  it("treats native file-picker cancellation as a silent cancellation", async () => {
    mocks.protocolPackageImport.mockResolvedValue(null);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);

    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入协议包 ZIP" })).not.toBeInTheDocument());
    expect(mocks.protocolPackageImportCommit).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows a complete no-source preview before enabling commit", async () => {
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));

    const preview = await screen.findByLabelText("协议包无源码预览");
    expect(preview).toHaveTextContent("iso-8583");
    expect(preview).toHaveTextContent("3.0.0");
    expect(preview).toHaveTextContent("2 个字段");
    expect(preview).toHaveTextContent("iso-message");
    expect(preview).toHaveTextContent("iso-response");
    expect(preview).toHaveTextContent("Socket");
    expect(preview).toHaveTextContent("可安装新版本");
    expect(preview).toHaveTextContent("默认停用");
    expect(preview).toHaveTextContent("上行 Encode：支持");
    expect(preview).toHaveTextContent("下行 Encode：支持");
    expect(within(preview).queryByText(/protocol\.rhai|manifest\.toml|脚本内容|absolute_path/i)).not.toBeInTheDocument();
    expect(within(preview).queryByText("018f-import-token")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确认安装" })).toBeEnabled();
    expect(mocks.protocolPackageImportCommit).not.toHaveBeenCalled();
  });

  it("fails closed for an incomplete prepare response", async () => {
    mocks.protocolPackageImport.mockResolvedValue(importPreview({ disposition: undefined as never }));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    expect(await screen.findByText("协议包校验预览数据不完整。")).toBeVisible();
    expect(mocks.protocolPackageImportCommit).not.toHaveBeenCalled();
  });

  it("shows reusable and identity-conflict dispositions without guessing", async () => {
    const user = userEvent.setup();
    mocks.protocolPackageImport.mockResolvedValueOnce(importPreview({ disposition: "reusable" }));
    const first = render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    expect(await screen.findByText("可复用精确版本")).toBeVisible();
    expect(screen.getByRole("button", { name: "确认安装" })).toBeEnabled();
    first.unmount();

    mocks.protocolPackageImport.mockResolvedValueOnce(importPreview({ disposition: "identity_conflict", token: null }));
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    expect(await screen.findByText("精确身份内容冲突")).toBeVisible();
    expect(screen.queryByRole("button", { name: "确认安装" })).not.toBeInTheDocument();
    expect(mocks.protocolPackageImportCommit).not.toHaveBeenCalled();
  });

  it("discards a ready preview before closing and restores trigger focus", async () => {
    const discard = deferred<{ success: boolean; message: string }>();
    mocks.protocolPackageImportDiscard.mockReturnValue(discard.promise);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    const trigger = screen.getByRole("button", { name: "导入协议包 ZIP" });
    await user.click(trigger);
    await screen.findByRole("button", { name: "确认安装" });
    await user.click(screen.getByRole("button", { name: "取消" }));

    expect(mocks.protocolPackageImportDiscard).toHaveBeenCalledWith("018f-import-token");
    expect(screen.getByLabelText("正在释放导入预览")).toBeVisible();
    discard.resolve({ success: true, message: "已释放" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入协议包 ZIP" })).not.toBeInTheDocument());
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(mocks.protocolPackageImportCommit).not.toHaveBeenCalled();
  });

  it("keeps a preview recoverable when discard fails", async () => {
    mocks.protocolPackageImportDiscard.mockRejectedValueOnce(appError(
      "REGISTRY_BUSY",
      "预览释放失败",
      "请稍后重试",
    ));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "取消" }));

    expect(await screen.findByText(/导入预览释放失败/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "确认安装" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试释放并关闭" })).toBeEnabled();
    mocks.protocolPackageImportDiscard.mockResolvedValueOnce({ success: true, message: "已释放" });
    await user.click(screen.getByRole("button", { name: "重试释放并关闭" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入协议包 ZIP" })).not.toBeInTheDocument());
    expect(mocks.protocolPackageImportDiscard).toHaveBeenCalledTimes(2);
  });

  it("closes safely when discard reports an expired or consumed token", async () => {
    mocks.protocolPackageImportDiscard.mockRejectedValueOnce(appError(
      "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID",
      "导入确认已过期",
      "请重新选择 ZIP",
    ));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    const trigger = screen.getByRole("button", { name: "导入协议包 ZIP" });
    await user.click(trigger);
    await user.click(await screen.findByRole("button", { name: "取消" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "导入协议包 ZIP" })).not.toBeInTheDocument());
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(mocks.protocolPackageImportDiscard).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["读取失败", "IO_ERROR", "无法读取协议包 ZIP", "/tmp/package.zip"],
    ["ZIP 非法", "INVALID_ZIP", "协议包不是合法 ZIP", "archive.zip: central directory"],
    ["Manifest 非法", "MANIFEST_INVALID", "Manifest 校验失败", "manifest.toml:12:7"],
    ["Schema 非法", "DOCUMENT_SCHEMA_INVALID", "Schema 校验失败", "document.toml:8:3"],
    ["Rhai 语法错", "SCRIPT_SYNTAX_ERROR", "Rhai 编译失败", "protocol.rhai:21:9"],
    ["入口错误", "ENTRY_POINT_MISSING", "脚本入口不存在", "protocol.rhai: frame"],
    ["身份冲突", "PROTOCOL_PACKAGE_IDENTITY_CONFLICT", "相同身份内容不同", "iso-8583@3.0.0"],
  ])("keeps %s stable, accessible, and never commits", async (_name, code, message, position) => {
    mocks.protocolPackageImport.mockRejectedValue(appError(code, message, position));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveFocus();
    expect(alert).toHaveTextContent(code);
    expect(alert).toHaveTextContent(message);
    expect(alert).toHaveTextContent(position);
    expect(screen.queryByRole("button", { name: "确认安装" })).not.toBeInTheDocument();
    expect(mocks.protocolPackageImportCommit).not.toHaveBeenCalled();
  });

  it.each([
    ["installed", "协议包安装成功。"],
    ["reused", "相同协议包已存在，已复用精确版本。"],
  ] as const)("refreshes and opens the exact version after %s", async (outcome, notice) => {
    mocks.protocolPackageImportCommit.mockResolvedValue(importResult(outcome));
    mocks.protocolPackageList
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([importedGroup()]);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "确认安装" }));

    expect(await screen.findByRole("status")).toHaveTextContent(notice);
    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();
    expect(mocks.protocolPackageImportCommit).toHaveBeenCalledTimes(1);
    expect(mocks.protocolPackageImportCommit).toHaveBeenCalledWith("018f-import-token");
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(2);
    expect(mocks.protocolPackageDetail).toHaveBeenCalledWith({ id: "iso-8583", version: "3.0.0" });
    expect(screen.getByRole("button", { name: /3\.0\.0/ })).toHaveAttribute("aria-pressed", "true");
  });

  it("synchronously blocks duplicate prepare and commit clicks", async () => {
    const prepare = deferred<ReturnType<typeof importPreview>>();
    const commit = deferred<ReturnType<typeof importResult>>();
    mocks.protocolPackageImport.mockReturnValue(prepare.promise);
    mocks.protocolPackageImportCommit.mockReturnValue(commit.promise);
    mocks.protocolPackageList
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([importedGroup()]);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    const trigger = screen.getByRole("button", { name: "导入协议包 ZIP" });

    await Promise.all([user.click(trigger), user.click(trigger)]);
    expect(mocks.protocolPackageImport).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("正在选择并完整校验协议包 ZIP")).toBeVisible();
    prepare.resolve(importPreview());
    const install = await screen.findByRole("button", { name: "确认安装" });
    await Promise.all([user.click(install), user.click(install)]);
    expect(mocks.protocolPackageImportCommit).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "确认安装" })).not.toBeInTheDocument();
    commit.resolve(importResult());
    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();
  });

  it("consumes the ready token before a commit error and never offers commit again", async () => {
    mocks.protocolPackageImportCommit.mockRejectedValue(appError(
      "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID",
      "导入确认凭据已失效",
      "请重新选择 ZIP",
    ));
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "确认安装" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("协议包导入失败");
    expect(alert).toHaveTextContent("PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID");
    expect(screen.queryByRole("button", { name: "确认安装" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新选择 ZIP" })).toBeEnabled();
    expect(mocks.protocolPackageImportCommit).toHaveBeenCalledTimes(1);
  });

  it("rejects a commit response for another exact identity", async () => {
    mocks.protocolPackageImportCommit.mockResolvedValue({
      ...importResult(),
      version: {
        ...importResult().version,
        package: { id: "tlv", version: "3.0.0" },
      },
    });
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "确认安装" }));
    expect(await screen.findByText(/导入结果与已确认预览不一致/)).toBeVisible();
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(1);
  });

  it("separates an installed package refresh failure and only retries the list", async () => {
    mocks.protocolPackageList
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(appError("REGISTRY_BUSY", "注册表暂不可读", "稍后重试"))
      .mockResolvedValueOnce([importedGroup()]);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "确认安装" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("协议包已安装，但列表刷新失败");
    expect(alert).toHaveTextContent("注册表暂不可读");
    expect(screen.queryByRole("button", { name: "确认安装" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "重新选择 ZIP" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重试刷新列表" }));

    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();
    expect(mocks.protocolPackageImportCommit).toHaveBeenCalledTimes(1);
    expect(mocks.protocolPackageList).toHaveBeenCalledTimes(3);
  });

  it.each([
    ["损坏列表", null],
    ["缺少精确版本", [group()]],
  ])("fails closed when refresh returns %s", async (_name, refreshed) => {
    mocks.protocolPackageList.mockResolvedValueOnce([]).mockResolvedValueOnce(refreshed);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "确认安装" }));
    expect(await screen.findByText(/刷新后的协议包列表数据不完整|未找到刚安装的精确协议包版本/)).toBeVisible();
    expect(screen.getByRole("button", { name: "重试刷新列表" })).toBeEnabled();
  });

  it("discards an older initial list response after the committed refresh", async () => {
    const oldList = deferred<never[]>();
    mocks.protocolPackageList
      .mockReturnValueOnce(oldList.promise)
      .mockResolvedValueOnce([importedGroup()]);
    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(screen.getByRole("button", { name: "导入协议包 ZIP" }));
    await user.click(await screen.findByRole("button", { name: "确认安装" }));
    expect(await screen.findByRole("dialog", { name: "ISO 8583" })).toBeVisible();

    oldList.resolve([]);
    await user.click(screen.getByRole("button", { name: "关闭协议包详情" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "查看协议包 ISO 8583" })).toBeVisible());
  });
});
