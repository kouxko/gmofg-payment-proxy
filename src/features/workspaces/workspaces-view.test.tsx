// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspacesView } from "./workspaces-view";

const mocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), workspaceCreate: vi.fn(), workspaceValidate: vi.fn(), workspaceSave: vi.fn(),
  workspaceImport: vi.fn(), workspaceExport: vi.fn(), workspaceCopy: vi.fn(), workspaceSelect: vi.fn(), workspaceDelete: vi.fn(),
  applicationConfigurationImport: vi.fn(), applicationConfigurationExport: vi.fn(), toast: vi.fn(),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@heroui/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@heroui/react")>();
  return { ...actual, toast: mocks.toast };
});

const workspace = { id: "workspace-1", name: "API Lab", revision: 1, listeners: [], metadata_extractors: [], response_assertions: [], fault_presets: [], certificate_references: [] };
function ok<T>(data: T) { return Promise.resolve({ status: "ok" as const, data }); }

describe("Workspace CRUD surface", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.workspaceList.mockReturnValue(ok([{ id: "workspace-1", name: "API Lab", revision: 1, listener_count: 0, enabled_listener_count: 0, selected: true }]));
    mocks.workspaceGet.mockReturnValue(ok(workspace));
    mocks.workspaceCreate.mockImplementation((name) => ok({ ...workspace, id: "workspace-2", name }));
    mocks.workspaceValidate.mockImplementation((draft) => ok({ valid: true, normalized: draft, field_errors: {} }));
    mocks.workspaceSave.mockImplementation((draft) => ok({ ...draft, revision: 2 }));
    mocks.workspaceSelect.mockReturnValue(ok({ id: "workspace-1", name: "API Lab", revision: 1, listener_count: 0, enabled_listener_count: 0, selected: true }));
    mocks.workspaceImport.mockReturnValue(ok({ message: "完整 Workspace 已导入", cancelled: false }));
    mocks.workspaceExport.mockReturnValue(ok({ message: "完整 Workspace 已导出", cancelled: false }));
    mocks.applicationConfigurationImport.mockReturnValue(ok({ message: "完整应用配置已导入", cancelled: false, ui_tone: "positive" }));
    mocks.applicationConfigurationExport.mockReturnValue(ok({ message: "完整应用配置已导出", cancelled: false, ui_tone: "positive" }));
  });

  it("creates a Workspace through the generated Rust command", async () => {
    const user = userEvent.setup();
    render(<WorkspacesView />);
    const input = await screen.findByRole("textbox", { name: "新 Workspace 名称" });
    await user.type(input, "Staging Lab");
    await user.click(screen.getByRole("button", { name: "新建" }));
    await waitFor(() => expect(mocks.workspaceCreate).toHaveBeenCalledWith("Staging Lab"));
  });

  it("validates before saving a renamed Workspace", async () => {
    const user = userEvent.setup();
    render(<WorkspacesView />);
    const input = await screen.findByRole("textbox", { name: "Workspace 名称" });
    await user.clear(input);
    await user.type(input, "Renamed Lab");
    await user.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(mocks.workspaceValidate).toHaveBeenCalledTimes(1));
    expect(mocks.workspaceSave.mock.calls[0][0].name).toBe("Renamed Lab");
  });

  it("explains that switching Workspace keeps running resources alive", async () => {
    const user = userEvent.setup();
    render(<WorkspacesView />);

    expect(await screen.findByText("切换只改变编辑上下文")).toBeVisible();
    expect(screen.getByText(/已运行的代理入口和设备网络接管不会自动停止/)).toBeVisible();
    await user.click(await screen.findByRole("button", { name: "设为当前 Workspace" }));
    await waitFor(() => expect(mocks.workspaceSelect).toHaveBeenCalledWith("workspace-1"));
  });

  it("明确完整 Workspace 导入导出及敏感材料边界", async () => {
    const user = userEvent.setup();
    render(<WorkspacesView />);

    const importButton = screen.getByRole("button", { name: "导入单个 Workspace" });
    const exportButton = await screen.findByRole("button", { name: "导出当前 Workspace" });
    expect(importButton).toBeVisible();
    expect(exportButton).toBeVisible();
    expect(screen.getByText(/完整应用配置还包含全部 Workspace/)).toBeVisible();
    expect(screen.getByText(/文件可能包含 Listener 外部证书、服务端私钥、PKCS12\/PFX 原文及明文密码/)).toBeVisible();
    expect(screen.getByText(/绝不包含本机 MITM Root CA 私钥/)).toBeVisible();

    await user.click(importButton);
    await waitFor(() => expect(mocks.workspaceImport).toHaveBeenCalledTimes(1));
    await user.click(exportButton);
    expect(await screen.findByRole("heading", { name: "导出当前 Workspace 的敏感配置？" })).toBeVisible();
    expect(mocks.workspaceExport).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "确认导出当前 Workspace" }));
    await waitFor(() => expect(mocks.workspaceExport).toHaveBeenCalledWith("workspace-1"));

    await user.click(screen.getByRole("button", { name: "导出完整应用配置" }));
    expect(await screen.findByRole("heading", { name: "导出完整应用配置的敏感内容？" })).toBeVisible();
    expect(mocks.applicationConfigurationExport).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "确认导出完整应用配置" }));
    await waitFor(() => expect(mocks.applicationConfigurationExport).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("button", { name: "导入完整应用配置" }));
    expect(await screen.findByRole("heading", { name: "替换全部应用配置？" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认选择文件并替换" }));
    await waitFor(() => expect(mocks.applicationConfigurationImport).toHaveBeenCalledTimes(1));
  });

  it("完整配置提交成功但清理不完整时显示 Rust 返回的警告语义", async () => {
    mocks.applicationConfigurationImport.mockReturnValue(ok({
      message: "配置已导入，但旧证书清理不完整",
      cancelled: false,
      ui_tone: "warning",
    }));
    const user = userEvent.setup();
    render(<WorkspacesView />);

    await user.click(await screen.findByRole("button", { name: "导入完整应用配置" }));
    await user.click(screen.getByRole("button", { name: "确认选择文件并替换" }));

    await waitFor(() => expect(mocks.toast).toHaveBeenCalledWith(
      "配置已导入，但旧证书清理不完整",
      { variant: "warning" },
    ));
  });
});
