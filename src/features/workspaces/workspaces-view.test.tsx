// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspacesView } from "./workspaces-view";

const mocks = vi.hoisted(() => ({
  workspaceList: vi.fn(), workspaceGet: vi.fn(), workspaceCreate: vi.fn(), workspaceValidate: vi.fn(), workspaceSave: vi.fn(),
  workspaceImport: vi.fn(), workspaceExport: vi.fn(), workspaceCopy: vi.fn(), workspaceSelect: vi.fn(), workspaceDelete: vi.fn(),
  applicationConfigurationImport: vi.fn(), applicationConfigurationExport: vi.fn(),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));

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
    mocks.workspaceImport.mockReturnValue(ok({ message: "完整 Workspace 已导入", cancelled: false }));
    mocks.workspaceExport.mockReturnValue(ok({ message: "完整 Workspace 已导出", cancelled: false }));
    mocks.applicationConfigurationImport.mockReturnValue(ok({ message: "完整应用配置已导入", cancelled: false }));
    mocks.applicationConfigurationExport.mockReturnValue(ok({ message: "完整应用配置已导出", cancelled: false }));
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

  it("明确完整 Workspace 导入导出及敏感材料边界", async () => {
    const user = userEvent.setup();
    render(<WorkspacesView />);

    const importButton = screen.getByRole("button", { name: "导入单个 Workspace" });
    const exportButton = await screen.findByRole("button", { name: "导出当前 Workspace" });
    expect(importButton).toBeVisible();
    expect(exportButton).toBeVisible();
    expect(screen.getByText(/完整应用配置还包含全部 Workspace/)).toBeVisible();
    expect(screen.getByText(/私钥与密码.*不会写入文件/)).toBeVisible();

    await user.click(importButton);
    await waitFor(() => expect(mocks.workspaceImport).toHaveBeenCalledTimes(1));
    await user.click(exportButton);
    await waitFor(() => expect(mocks.workspaceExport).toHaveBeenCalledWith("workspace-1"));

    await user.click(screen.getByRole("button", { name: "导出完整应用配置" }));
    await waitFor(() => expect(mocks.applicationConfigurationExport).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole("button", { name: "导入完整应用配置" }));
    expect(await screen.findByRole("heading", { name: "替换全部应用配置？" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认选择文件并替换" }));
    await waitFor(() => expect(mocks.applicationConfigurationImport).toHaveBeenCalledTimes(1));
  });
});
