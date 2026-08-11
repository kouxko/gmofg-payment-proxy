// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  deferred,
  mocks,
  ok,
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

  it("imports one Workspace, reports success, then refreshes the list", async () => {
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "导入单个 Workspace" }));

    await waitFor(() => expect(mocks.workspaceImport).toHaveBeenCalledTimes(1));
    expect(mocks.toast).toHaveBeenCalledWith("完整 Workspace 已导入", {
      variant: "success",
    });
    await waitFor(() =>
      expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls + 1),
    );
    expect(mocks.toast.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.workspaceList.mock.invocationCallOrder.at(-1)!,
    );
  });

  it("reports a cancelled Workspace import without claiming success", async () => {
    mocks.workspaceImport.mockReturnValue(
      ok({ message: "未选择 Workspace 文件", cancelled: true }),
    );
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导入单个 Workspace" }));

    await waitFor(() =>
      expect(mocks.toast).toHaveBeenCalledWith("未选择 Workspace 文件", {
        variant: "default",
      }),
    );
  });

  it("does not export a Workspace when the confirmation is cancelled", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导出当前 Workspace" }));
    expect(
      await screen.findByRole("heading", {
        name: "导出当前 Workspace 的敏感配置？",
      }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "取消" }));

    await waitFor(() =>
      expect(
        screen.queryByRole("heading", {
          name: "导出当前 Workspace 的敏感配置？",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.workspaceExport).not.toHaveBeenCalled();
  });

  it("exports the selected Workspace only after confirmation", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导出当前 Workspace" }));
    await user.click(
      await screen.findByRole("button", {
        name: "确认导出当前 Workspace",
      }),
    );

    await waitFor(() =>
      expect(mocks.workspaceExport).toHaveBeenCalledWith("workspace-1"),
    );
    expect(mocks.toast).toHaveBeenCalledWith("完整 Workspace 已导出", {
      variant: "success",
    });
  });

  it("does not replace the application configuration when import is cancelled", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导入完整应用配置" }));
    expect(
      await screen.findByRole("heading", { name: "替换全部应用配置？" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "取消" }));

    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "替换全部应用配置？" }),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.applicationConfigurationImport).not.toHaveBeenCalled();
  });

  it("imports the full configuration, reports its Rust tone, then refreshes", async () => {
    mocks.applicationConfigurationImport.mockReturnValue(
      ok({
        message: "配置已导入，但旧证书清理不完整",
        cancelled: false,
        ui_tone: "warning",
      }),
    );
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "导入完整应用配置" }));
    await user.click(
      screen.getByRole("button", { name: "确认选择文件并替换" }),
    );

    await waitFor(() =>
      expect(mocks.applicationConfigurationImport).toHaveBeenCalledTimes(1),
    );
    expect(mocks.toast).toHaveBeenCalledWith(
      "配置已导入，但旧证书清理不完整",
      { variant: "warning" },
    );
    await waitFor(() =>
      expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls + 1),
    );
    expect(mocks.toast.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.workspaceList.mock.invocationCallOrder.at(-1)!,
    );
  });

  it("does not export the full configuration when confirmation is cancelled", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导出完整应用配置" }));
    expect(
      await screen.findByRole("heading", {
        name: "导出完整应用配置的敏感内容？",
      }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "取消" }));

    await waitFor(() =>
      expect(
        screen.queryByRole("heading", {
          name: "导出完整应用配置的敏感内容？",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.applicationConfigurationExport).not.toHaveBeenCalled();
  });

  it("exports the full configuration only after confirmation", async () => {
    const user = userEvent.setup();
    await renderLoadedView();

    await user.click(screen.getByRole("button", { name: "导出完整应用配置" }));
    await user.click(
      await screen.findByRole("button", {
        name: "确认导出完整应用配置",
      }),
    );

    await waitFor(() =>
      expect(mocks.applicationConfigurationExport).toHaveBeenCalledTimes(1),
    );
    expect(mocks.toast).toHaveBeenCalledWith("完整应用配置已导出", {
      variant: "success",
    });
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
    const pendingImport = deferred<{
      status: "ok";
      data: { message: string; cancelled: boolean };
    }>();
    mocks.workspaceImport.mockReturnValue(pendingImport.promise);
    const user = userEvent.setup();
    await renderLoadedView();

    const importButton = screen.getByRole("button", {
      name: "导入单个 Workspace",
    });
    await user.click(importButton);

    expect(importButton).toBeDisabled();
    expect(screen.getByRole("button", { name: "新建" })).toBeDisabled();
    await user.click(importButton);
    expect(mocks.workspaceImport).toHaveBeenCalledTimes(1);

    pendingImport.resolve({
      status: "ok",
      data: { message: "完整 Workspace 已导入", cancelled: false },
    });
    await waitFor(() => expect(importButton).toBeEnabled());
  });

  it("waits for an action to finish before showing its toast and refreshing", async () => {
    const pendingImport = deferred<{
      status: "ok";
      data: { message: string; cancelled: boolean };
    }>();
    mocks.workspaceImport.mockReturnValue(pendingImport.promise);
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "导入单个 Workspace" }));

    expect(mocks.toast).not.toHaveBeenCalled();
    expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls);

    pendingImport.resolve({
      status: "ok",
      data: { message: "完整 Workspace 已导入", cancelled: false },
    });
    await waitFor(() => expect(mocks.toast).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls + 1),
    );
    expect(mocks.toast.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.workspaceList.mock.invocationCallOrder.at(-1)!,
    );
  });

  it("shows IPC failures as danger toasts without refreshing", async () => {
    mocks.workspaceCopy.mockRejectedValue(new Error("复制失败"));
    const user = userEvent.setup();
    await renderLoadedView();
    const initialListCalls = mocks.workspaceList.mock.calls.length;

    await user.click(screen.getByRole("button", { name: "复制" }));

    await waitFor(() =>
      expect(mocks.toast).toHaveBeenCalledWith(
        "无法连接 Rust 核心，请确认桌面应用已完成初始化。",
        { variant: "danger" },
      ),
    );
    expect(mocks.workspaceList).toHaveBeenCalledTimes(initialListCalls);
  });

  it("explains the portable files' sensitive-material boundary", async () => {
    await renderLoadedView();

    expect(screen.getByText(/完整应用配置还包含全部 Workspace/)).toBeVisible();
    expect(
      screen.getByText(
        /文件可能包含 Listener 外部证书、服务端私钥、PKCS12\/PFX 原文及明文密码/,
      ),
    ).toBeVisible();
    expect(screen.getByText(/绝不包含本机 MITM Root CA 私钥/)).toBeVisible();
  });
});
