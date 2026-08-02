// @vitest-environment jsdom

/** 验证设置草稿、字段错误、保存/重启门禁与恢复默认值确认。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SettingsDraft,
  SettingsViewModel,
} from "@/generated/rust-types";
import { SettingsView } from "./settings-view";

const commandMocks = vi.hoisted(() => ({
  settingsResetDefaults: vi.fn(),
  settingsSave: vi.fn(),
}));

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
  leaf_sans: ["127.0.0.1"],
};

const settings: SettingsViewModel = {
  stored: draft,
  effective: draft,
  pending_changes: false,
  requires_restart: false,
  restart_reason: null,
  revision: 1,
  can_write: true,
  disabled_reason: null,
  fixed_tls_version: "TLS 1.2",
  redirects_enabled: false,
  retries_enabled: false,
  payload_policy_text: "Payload 仅保存在内存中。",
};

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: () => ({
    data: settings,
    error: undefined,
    isLoading: false,
    refresh: vi.fn(),
    setData: vi.fn(),
  }),
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

describe("production SettingsView overlay", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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

  it("keeps listener addresses, upstream targets and lifecycle out of system settings", () => {
    render(<SettingsView />);

    expect(screen.queryByText("通道 ID：transaction")).not.toBeInTheDocument();
    expect(screen.queryByText("通道 ID：dll")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("服务端证书 SAN")).not.toBeInTheDocument();
    expect(screen.queryByText("上游 URL")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "保存并重启代理" })).not.toBeInTheDocument();
    expect(screen.getByText(/代理入口的监听地址、端口、上游和 TLS/)).toBeVisible();
  });

  it("switches setting tabs without replacing the page document", async () => {
    const user = userEvent.setup();
    const { container } = render(<SettingsView />);
    const viewRoot = container.firstElementChild;
    const locationBefore = window.location.href;

    await user.click(screen.getByRole("tab", { name: "数据与导出" }));

    expect(screen.getByText(settings.payload_policy_text)).toBeVisible();
    expect(container.firstElementChild).toBe(viewRoot);
    expect(window.location.href).toBe(locationBefore);
  });
});
