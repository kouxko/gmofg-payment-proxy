// @vitest-environment jsdom

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

  it(
    "sends raw SAN text to Rust without TypeScript normalization",
    async () => {
      commandMocks.settingsSave.mockResolvedValue(settings);
      const user = userEvent.setup();
      render(<SettingsView />);

      const raw = " Proxy.Local，10.0.34.50, proxy.local ";
      await user.clear(
        screen.getByRole("textbox", { name: "服务端证书 SAN" }),
      );
      await user.type(
        screen.getByRole("textbox", { name: "服务端证书 SAN" }),
        raw,
      );
      await user.click(screen.getByRole("button", { name: "保存设置" }));

      expect(commandMocks.settingsSave).toHaveBeenCalledWith(
        expect.any(Object),
        raw,
      );
    },
    10_000,
  );

  it("renders channel editors from the Rust settings catalog", () => {
    render(<SettingsView />);

    expect(screen.getByText("通道 ID：transaction")).toBeVisible();
    expect(screen.getByText("通道 ID：dll")).toBeVisible();
    expect(
      screen.getByRole("switch", { name: "启用交易" }),
    ).toBeChecked();
    expect(screen.getByRole("switch", { name: "启用DLL" })).toBeChecked();
  });

  it("switches setting tabs without replacing the page document", async () => {
    const user = userEvent.setup();
    const { container } = render(<SettingsView />);
    const viewRoot = container.firstElementChild;
    const locationBefore = window.location.href;

    await user.click(screen.getByRole("tab", { name: "超时与容量" }));

    expect(screen.getByRole("textbox", { name: "最大会话数" })).toBeVisible();
    expect(container.firstElementChild).toBe(viewRoot);
    expect(window.location.href).toBe(locationBefore);
  });
});
