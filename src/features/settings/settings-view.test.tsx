// @vitest-environment jsdom

/** 验证设置草稿、字段错误、保存/重启门禁与恢复默认值确认。 */

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  McpInfoViewModel,
  ExternalPackageServiceStatusViewModel,
  SettingsDraft,
  SettingsViewModel,
} from "@/generated/rust-types";
import { SettingsView } from "./settings-view";
import { isExternalPackageServiceStatus } from "./external-package-service-settings";

const commandMocks = vi.hoisted(() => ({
  applicationDataReset: vi.fn(),
  settingsResetDefaults: vi.fn(),
  settingsSave: vi.fn(),
  settingsValidate: vi.fn(),
  settingsGet: vi.fn(),
  mcpInfo: vi.fn(),
  externalPackageServiceStatus: vi.fn(),
  settingsSetData: vi.fn(),
}));
const appEventRefreshMock = vi.hoisted(() => vi.fn());

vi.mock("@/generated/rust-types", () => ({
  commands: commandMocks,
}));

vi.mock("@/lib/ipc/client", () => ({
  appErrorViewModel: () => undefined,
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: () => "Rust 操作失败",
}));

const draft: SettingsDraft = {
  expected_revision: 1,
  bind_address: "0.0.0.0",
  channels: [
    {
      id: "transaction",
      display_name: "交易",
      enabled: true,
      port: 16627,
      upstream_url: "https://transaction.example.test",
    },
    {
      id: "dll",
      display_name: "DLL",
      enabled: true,
      port: 16127,
      upstream_url: "https://dll.example.test",
    },
  ],
  connect_timeout_seconds: 70,
  write_timeout_seconds: 70,
  read_timeout_seconds: 70,
  rewrite_host: true,
  max_body_bytes: 4 * 1024 * 1024,
  max_sessions: 500,
  max_memory_bytes: 256 * 1024 * 1024,
  ui_event_capacity: 4096,
  leaf_sans: ["127.0.0.1"],
  external_package_service: {
    bind_address: "0.0.0.0",
    port: 8765,
    rpc_timeout_seconds: 5,
    max_in_flight: 256,
  },
};

const settings: SettingsViewModel = {
  stored: draft,
  revision: 1,
  can_write: true,
  disabled_reason: null,
  fixed_tls_version: "TLS 1.2",
  redirects_enabled: false,
  retries_enabled: false,
  payload_policy_text: "Payload 仅保存在内存中。",
};

const mcpInfo: McpInfoViewModel = {
  available: true,
  endpoint: "http://127.0.0.1:17653/mcp",
  protocol_version: "2026-07-28",
  transport: "Streamable HTTP",
  access_scope: "仅本机回环地址",
  authentication: "无认证",
  tool_count: 33,
  resource_count: 11,
};

const externalStatus: ExternalPackageServiceStatusViewModel = {
  websocket_url: "ws://0.0.0.0:8765/packages",
  fixed_path: "/packages",
  online_connection_count: 2,
  state: { state: "listening" },
  authentication_enabled: false,
};

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) => {
    const [data, setData] = useState(key === "mcp-info"
      ? mcpInfo
      : key === "external-package-service-status" ? externalStatus : settings);
    return {
      data,
      error: undefined,
      isLoading: false,
      refresh: vi.fn(),
      setData: (next: SettingsViewModel | McpInfoViewModel | ExternalPackageServiceStatusViewModel) => {
        commandMocks.settingsSetData(next);
        setData(next);
      },
    };
  },
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: appEventRefreshMock,
}));

describe("production SettingsView overlay", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    commandMocks.settingsValidate.mockResolvedValue({
      valid: true,
      field_errors: {},
      warnings: ["Rust 校验警告"],
    });
    commandMocks.settingsSave.mockResolvedValue({
      ...settings,
      stored: { ...draft, max_sessions: 501 },
    });
  });

  it("refreshes the external package service card from authoritative service events", async () => {
    const user = userEvent.setup();
    render(<SettingsView />);
    await user.click(screen.getByRole("tab", { name: "外部软件包" }));

    expect(appEventRefreshMock).toHaveBeenCalledWith(
      ["external_package_service_status_changed", "snapshot_required"],
      expect.any(Function),
    );
  });

  it("does not expose manual validation controls or validation-result landmarks", () => {
    render(<SettingsView />);
    expect(screen.queryByText("校验结果")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "校验设置" })).not.toBeInTheDocument();
  });

  it("fails closed for malformed external service runtime snapshots", () => {
    expect(isExternalPackageServiceStatus(externalStatus)).toBe(true);
    expect(isExternalPackageServiceStatus({ ...externalStatus, fixed_path: "/legacy" })).toBe(false);
    expect(isExternalPackageServiceStatus({ ...externalStatus, authentication_enabled: undefined })).toBe(false);
    expect(isExternalPackageServiceStatus({ ...externalStatus, online_connection_count: -1 })).toBe(false);
    expect(isExternalPackageServiceStatus({ ...externalStatus, legacy_endpoint: true })).toBe(false);
  });

  it("saves through Rust and replaces the displayed stored snapshot", async () => {
    const user = userEvent.setup();
    render(<SettingsView />);

    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(commandMocks.settingsSave).toHaveBeenCalledWith(draft),
    );
    expect(commandMocks.settingsValidate).toHaveBeenCalledWith(draft);
    expect(commandMocks.settingsSetData).toHaveBeenCalledWith(
      expect.objectContaining({
        stored: expect.objectContaining({ max_sessions: 501 }),
      }),
    );
  });

  it("keeps the real reset AlertDialog open while Rust is pending", async () => {
    let finish!: (value: SettingsDraft) => void;
    commandMocks.settingsResetDefaults.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<SettingsView />);

    await user.click(screen.getByRole("button", { name: "恢复默认值" }));
    expect(
      screen.getByRole("alertdialog", { name: "恢复默认设置草稿？" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认恢复" }));

    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "正在恢复…" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(
      screen.getByRole("alertdialog", { name: "恢复默认设置草稿？" }),
    ).toBeVisible();

    finish(draft);
    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", {
          name: "恢复默认设置草稿？",
        }),
      ).not.toBeInTheDocument(),
    );
  });

  it("requires destructive confirmation before clearing all persisted data", async () => {
    let finish!: () => void;
    commandMocks.applicationDataReset.mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<SettingsView />);

    await user.click(
      screen.getByRole("button", { name: "清除全部配置与数据" }),
    );
    expect(
      screen.getByRole("alertdialog", { name: "清除全部配置与测试数据？" }),
    ).toBeVisible();
    expect(commandMocks.applicationDataReset).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "确认清除并重启" }));
    expect(commandMocks.applicationDataReset).toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "正在清除并重启…" }),
    ).toBeDisabled();

    finish();
    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", {
          name: "清除全部配置与测试数据？",
        }),
      ).not.toBeInTheDocument(),
    );
  });

  it("keeps listener addresses, upstream targets and lifecycle out of system settings", () => {
    render(<SettingsView />);

    expect(screen.queryByText("通道 ID：transaction")).not.toBeInTheDocument();
    expect(screen.queryByText("通道 ID：dll")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("服务端证书 SAN")).not.toBeInTheDocument();
    expect(screen.queryByText("上游 URL")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "保存并重启代理" })).not.toBeInTheDocument();
    expect(screen.getByText(/代理入口的监听地址、端口、上游和 TLS/)).toBeVisible();
  });

  it("keeps the compact settings tabs without the summary sidebar", () => {
    render(<SettingsView />);

    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "超时与容量",
      "应用",
      "外部软件包",
      "AI 助手（MCP）",
    ]);
    expect(screen.queryByText("数据与导出")).not.toBeInTheDocument();
    expect(screen.queryByText("配置摘要与校验")).not.toBeInTheDocument();
  });

  it("shows the MCP address, client configuration and App-side guidance", async () => {
    const user = userEvent.setup();
    render(<SettingsView />);

    await user.click(screen.getByRole("tab", { name: "AI 助手（MCP）" }));

    expect(screen.getByText("http://127.0.0.1:17653/mcp")).toBeVisible();
    expect(screen.getByText("33 个只读工具 · 11 个参考资源")).toBeVisible();
    expect(screen.getByText(/App 端的修改建议/)).toBeVisible();
    expect(screen.getByText(/Root CA、服务端证书、客户端证书/)).toBeVisible();
    expect(screen.getByText(/"type": "http"/)).toBeVisible();
  });

  it("shows the authoritative external package service status and restart boundary", async () => {
    const user = userEvent.setup();
    render(<SettingsView />);

    await user.click(screen.getByRole("tab", { name: "外部软件包" }));

    expect(screen.getByText("正在监听")).toBeVisible();
    expect(screen.getByText("ws://0.0.0.0:8765/packages")).toBeVisible();
    expect(screen.getByText("/packages")).toBeVisible();
    expect(screen.getByText("2 个")).toBeVisible();
    expect(screen.getByText("当前版本不提供连接认证")).toBeVisible();
    expect(screen.getByText(/重启应用后生效/)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "监听地址" })).toHaveValue("0.0.0.0");
    expect(screen.getByRole("textbox", { name: "端口" })).toHaveValue("8,765");
    expect(screen.getByRole("textbox", { name: "RPC 超时（秒）" })).toHaveValue("5");
    expect(screen.getByRole("textbox", { name: "最大并发 RPC" })).toHaveValue("256");
    expect(screen.getByRole("button", { name: "保存设置" })).toBeVisible();
  });

  it("does not write when Rust validation rejects the current draft", async () => {
    commandMocks.settingsValidate.mockResolvedValue({
      valid: false,
      field_errors: { max_sessions: ["最大会话数无效"] },
      warnings: [],
    });
    const user = userEvent.setup();
    render(<SettingsView />);

    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() =>
      expect(screen.getByText("最大会话数无效")).toBeVisible(),
    );
    expect(commandMocks.settingsSave).not.toHaveBeenCalled();
  });

  it("shows only unmapped validation errors in the page-level alert", async () => {
    commandMocks.settingsValidate.mockResolvedValue({
      valid: false,
      field_errors: { bind_address: ["监听地址由入口配置管理"] },
      warnings: [],
    });
    const user = userEvent.setup();
    render(<SettingsView />);
    await user.click(screen.getByRole("button", { name: "保存设置" }));
    expect(await screen.findByText("设置无法保存")).toBeVisible();
    expect(screen.getByText("监听地址由入口配置管理")).toBeVisible();
  });

  it("shows compact saved and dirty draft states", async () => {
    commandMocks.settingsSave.mockResolvedValue({
      ...settings,
      stored: { ...draft, connect_timeout_seconds: 71 },
    });
    const user = userEvent.setup();
    render(<SettingsView />);
    expect(screen.getByText("已保存")).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Increase 连接超时（秒）" }),
    );
    expect(screen.getByText("有未保存更改")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "保存设置" }));
    expect(await screen.findByText("已保存")).toBeVisible();
  });

  it("disables Rust-backed fields while save validation is pending", async () => {
    let finish!: (value: { valid: boolean; field_errors: object; warnings: string[] }) => void;
    commandMocks.settingsValidate.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const user = userEvent.setup();
    render(<SettingsView />);
    await user.click(screen.getByRole("button", { name: "保存设置" }));
    expect(screen.getByRole("textbox", { name: "连接超时（秒）" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Host 头重写为目标主机" })).toBeDisabled();
    finish({ valid: false, field_errors: {}, warnings: [] });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "保存设置" })).toBeEnabled(),
    );
  });
});
