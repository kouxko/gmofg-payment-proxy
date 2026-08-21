// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  deferred,
  mocks,
  setupWorkspaceMocks,
  workspace,
  workspaceSummary,
} from "./workspaces-view.test-support";
import { WorkspacesView } from "./workspaces-view";

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@heroui/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@heroui/react")>();
  return { ...actual, toast: mocks.toast };
});

async function renderLoadedView() {
  render(<WorkspacesView />);
  await screen.findByRole("textbox", { name: "Workspace 名称" });
}

describe("Workspace CRUD surface", () => {
  beforeEach(setupWorkspaceMocks);

  it("creates the named Workspace and refreshes the list", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    const input = screen.getByRole("textbox", { name: "新 Workspace 名称" });
    await user.type(input, "Staging Lab");
    await user.click(screen.getByRole("button", { name: "新建" }));

    await waitFor(() =>
      expect(mocks.workspaceCreate).toHaveBeenCalledWith("Staging Lab"),
    );
    await waitFor(() =>
      expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls + 1),
    );
    expect(input).toHaveValue("");
  });

  it("validates before saving a renamed Workspace", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    const input = screen.getByRole("textbox", { name: "Workspace 名称" });

    await user.clear(input);
    await user.type(input, "Renamed Lab");
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(mocks.workspaceValidate).toHaveBeenCalledTimes(1));
    expect(mocks.workspaceSave.mock.calls[0][0].name).toBe("Renamed Lab");
  });

  it("locks draft edits only until save completes, not until background refresh completes", async () => {
    const pendingSave = deferred<{
      status: "ok";
      data: typeof workspace;
    }>();
    const pendingRefresh = deferred<{
      status: "ok";
      data: typeof workspaceSummary[];
    }>();
    mocks.workspaceSave.mockReturnValue(pendingSave.promise);
    const user = userEvent.setup();
    await renderLoadedView();

    const nameInput = screen.getByRole("textbox", { name: "Workspace 名称" });
    const saveButton = screen.getByRole("button", { name: "保存" });
    await user.click(saveButton);
    await waitFor(() => expect(mocks.workspaceSave).toHaveBeenCalledOnce());
    expect(nameInput).toBeDisabled();

    mocks.workspaceList.mockReturnValue(pendingRefresh.promise);
    pendingSave.resolve({
      status: "ok",
      data: { ...workspace, revision: 2 },
    });

    await waitFor(() => expect(saveButton).toBeEnabled());
    expect(nameInput).toBeEnabled();
    expect(mocks.toast).toHaveBeenCalledWith("Workspace 已保存。", {
      variant: "success",
    });

    pendingRefresh.resolve({ status: "ok", data: [workspaceSummary] });
  });

  it("shows exactly the two ordinary application-data migration actions", async () => {
    await renderLoadedView();
    expect(screen.getByRole("button", { name: "导出应用数据" })).toBeVisible();
    expect(screen.getByRole("button", { name: "导入应用数据" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "导出当前 Workspace" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "导入单个 Workspace" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "导出完整应用配置" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "导入完整应用配置" })).not.toBeInTheDocument();
  });

  it("exports application data through the ZIP command", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    await user.click(screen.getByRole("button", { name: "导出应用数据" }));
    await waitFor(() => expect(mocks.applicationBackupExport).toHaveBeenCalledOnce());
    expect(mocks.toast).toHaveBeenCalledWith("应用数据已导出（2048 字节）。", {
      variant: "success",
    });
  });

  it("previews exact replacement scope before committing the ZIP", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    await user.click(screen.getByRole("button", { name: "导入应用数据" }));
    expect(await screen.findByRole("heading", { name: "确认替换应用数据？" })).toBeVisible();
    expect(screen.getByText(/2 个 Workspace · 3 个协议包版本/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认替换" }));
    await waitFor(() => expect(mocks.applicationBackupImportCommit).toHaveBeenCalledWith("backup-token"));
  });

  it("discards the prepared token when preview is cancelled", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    await user.click(screen.getByRole("button", { name: "导入应用数据" }));
    await user.click(await screen.findByRole("button", { name: "取消" }));
    await waitFor(() => expect(mocks.applicationBackupImportDiscard).toHaveBeenCalledWith("backup-token"));
    expect(mocks.applicationBackupImportCommit).not.toHaveBeenCalled();
  });

  it("selects the Workspace without implying running resources were stopped", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    expect(screen.getByText("切换只改变编辑上下文")).toBeVisible();
    expect(
      screen.getByText(/已运行的代理入口和设备网络接管不会自动停止/),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "设为当前 Workspace" }));

    await waitFor(() =>
      expect(mocks.workspaceSelect).toHaveBeenCalledWith("workspace-1"),
    );
    expect(mocks.toast).toHaveBeenCalledWith(
      "已切换当前 Workspace；运行中的代理入口和设备网络接管保持不变。",
      { variant: "success" },
    );
  });

  it("copies the selected Workspace and refreshes the list", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "复制" }));

    await waitFor(() =>
      expect(mocks.workspaceCopy).toHaveBeenCalledWith("workspace-1"),
    );
    await waitFor(() =>
      expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls + 1),
    );
    expect(screen.getByRole("textbox", { name: "Workspace 名称" })).toHaveValue(
      "API Lab 副本",
    );
  });

  it("does not delete the Workspace when confirmation is cancelled", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "删除" }));
    expect(
      await screen.findByRole("heading", { name: "删除 API Lab？" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "取消" }));

    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "删除 API Lab？" }),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.workspaceDelete).not.toHaveBeenCalled();
  });

  it("deletes with the selected revision, reports success, then refreshes", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "删除" }));
    await user.click(
      await screen.findByRole("button", { name: "确认删除" }),
    );

    await waitFor(() =>
      expect(mocks.workspaceDelete).toHaveBeenCalledWith("workspace-1", 1),
    );
    expect(mocks.toast).toHaveBeenCalledWith("Workspace 已删除。", {
      variant: "success",
    });
    await waitFor(() =>
      expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls + 1),
    );
    expect(mocks.toast.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.workspaceList.mock.invocationCallOrder.at(-1)!,
    );
  });

  it("guards against a second action while an IPC command is pending", async () => {
    const pendingImport = deferred<Awaited<ReturnType<typeof mocks.applicationBackupImportPrepare>>>();
    mocks.applicationBackupImportPrepare.mockReturnValue(pendingImport.promise);
    const user = userEvent.setup();
    await renderLoadedView();

    const importButton = screen.getByRole("button", {
      name: "导入应用数据",
    });
    await user.click(importButton);

    expect(importButton).toBeDisabled();
    expect(screen.getByRole("button", { name: "新建" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "新 Workspace 名称" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "导出应用数据" })).toBeDisabled();
    await user.click(importButton);
    expect(mocks.applicationBackupImportPrepare).toHaveBeenCalledTimes(1);

    pendingImport.resolve({
      status: "ok",
      data: null,
    });
    await waitFor(() => expect(importButton).toBeEnabled());
  });

  it("keeps the workspace name input in its own shrinkable grid cell", async () => {
    await renderLoadedView();
    const toolbar = screen.getByTestId("workspace-toolbar");
    const input = screen.getByRole("textbox", { name: "新 Workspace 名称" });

    expect(toolbar).toHaveClass("grid", "min-w-0");
    expect(toolbar).not.toHaveClass("overflow-x-auto");
    expect(input).toHaveClass("w-full", "min-w-0");
    expect(toolbar.parentElement).toHaveClass("min-w-0");
    expect(toolbar.parentElement?.parentElement).toHaveClass("overflow-x-hidden");
  });

  it("waits for ZIP export to finish before showing success", async () => {
    const pendingExport = deferred<Awaited<ReturnType<typeof mocks.applicationBackupExport>>>();
    mocks.applicationBackupExport.mockReturnValue(pendingExport.promise);
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导出应用数据" }));

    expect(mocks.toast).not.toHaveBeenCalled();
    pendingExport.resolve({
      status: "ok",
      data: { bytes_written: 2048, replaced_existing: false },
    });
    await waitFor(() => expect(mocks.toast).toHaveBeenCalledTimes(1));
  });

  it("shows IPC failures as danger toasts without refreshing", async () => {
    mocks.workspaceCopy.mockRejectedValue(new Error("复制失败"));
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "复制" }));

    await waitFor(() =>
      expect(mocks.toast).toHaveBeenCalledWith(
        "无法连接应用核心，请确认桌面应用已完成初始化。",
        { variant: "danger" },
      ),
    );
    expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls);
  });

});
