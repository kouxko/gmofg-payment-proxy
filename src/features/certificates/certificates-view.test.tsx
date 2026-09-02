// @vitest-environment jsdom

/** 验证证书页面只提交用户意图、正确清除密码并显示 Rust 返回状态。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CertificatesView } from "./certificates-view";

const mocks = vi.hoisted(() => ({
  certificateExportCa: vi.fn(),
  certificateGenerateCa: vi.fn(),
  certificateOverview: vi.fn(),
  certificateReissueLeaf: vi.fn(),
  certificateResetCa: vi.fn(),
  certificateValidate: vi.fn(),
  settingsGet: vi.fn(),
  settingsSetData: vi.fn(),
  overviewRefresh: vi.fn().mockResolvedValue(undefined),
  navigate: vi.fn(),
}));

vi.mock("@/features/shell/workspace-navigation", () => ({
  useWorkspaceNavigation: () => ({ navigate: mocks.navigate }),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    certificateExportCa: mocks.certificateExportCa,
    certificateGenerateCa: mocks.certificateGenerateCa,
    certificateOverview: mocks.certificateOverview,
    certificateReissueLeaf: mocks.certificateReissueLeaf,
    certificateResetCa: mocks.certificateResetCa,
    certificateValidate: mocks.certificateValidate,
    settingsGet: mocks.settingsGet,
  },
}));

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => String(reason),
}));

vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

vi.mock("@/lib/ipc/use-ipc-query", () => ({
  useIpcQuery: (key: string) =>
    key === "certificate-overview"
      ? {
          data: {
            revision: 1,
            ready: false,
            status_text: "证书配置不完整",
            ui_tone: "warning",
            items: [
              {
                kind: "local_root_ca",
                subject: "CN=Intercept Proxy Root CA",
                usage: "签发本机代理服务端证书",
                sans: [],
                valid_from: "2026-08-01T00:00:00Z",
                valid_until: "2036-08-01T00:00:00Z",
                sha256_fingerprint: "AA:BB:CC:DD",
                status_text: "有效",
                ui_tone: "positive",
              },
              {
                kind: "proxy_leaf",
                subject: "CN=10.0.34.50",
                usage: "客户端连接本机代理时使用的服务端身份",
                sans: ["IP:10.0.34.50", "DNS:proxy.test"],
                valid_from: "2026-08-01T00:00:00Z",
                valid_until: "2028-08-01T00:00:00Z",
                sha256_fingerprint: "11:22:33:44",
                status_text: "有效",
                ui_tone: "positive",
              },
            ],
            can_initialize: true,
            can_change: true,
            disabled_reason: null,
          },
          error: undefined,
          isLoading: false,
          refresh: mocks.overviewRefresh,
          setData: vi.fn(),
        }
      : {
          data: { stored: { leaf_sans: ["old.proxy.test"] } },
          error: undefined,
          isLoading: false,
          refresh: vi.fn(),
          setData: mocks.settingsSetData,
        },
}));

describe("CertificatesView settings freshness", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.overviewRefresh.mockResolvedValue(undefined);
    mocks.settingsGet.mockResolvedValue({
      stored: { leaf_sans: ["10.0.28.99"] },
    });
    mocks.certificateGenerateCa.mockResolvedValue({
      revision: 2,
      ready: false,
      status_text: "测试证书已初始化",
      ui_tone: "positive",
      items: [],
      can_initialize: false,
      can_change: true,
      disabled_reason: null,
    });
    mocks.certificateExportCa.mockResolvedValue({
      success: true,
      cancelled: false,
      message: "本机公开 Root CA 已导出，未包含私钥。",
      ui_tone: "positive",
      entity_id: null,
      revision: null,
      requires_restart: false,
    });
    mocks.certificateReissueLeaf.mockResolvedValue({
      revision: 2,
      ready: true,
      status_text: "服务端证书已重新签发",
      ui_tone: "positive",
      items: [],
      can_initialize: false,
      can_change: true,
      disabled_reason: null,
    });
    mocks.certificateResetCa.mockResolvedValue({
      revision: 2,
      ready: true,
      status_text: "本机证书已重置",
      ui_tone: "positive",
      items: [],
      can_initialize: false,
      can_change: true,
      disabled_reason: null,
    });
    mocks.certificateValidate.mockResolvedValue({
      valid: true,
      field_errors: {},
      warnings: ["证书即将过期"],
    });
  });

  it("reads the latest Rust settings immediately before generating a leaf", async () => {
    const user = userEvent.setup();
    render(<CertificatesView />);

    await user.click(
      screen.getByRole("button", { name: "初始化本机证书" }),
    );

    await waitFor(() =>
      expect(mocks.certificateGenerateCa).toHaveBeenCalledWith([
        "10.0.28.99",
      ]),
    );
    expect(mocks.settingsSetData).toHaveBeenCalledWith(
      expect.objectContaining({
        stored: { leaf_sans: ["10.0.28.99"] },
      }),
    );
  });

  it("exports only the installation public Root CA", async () => {
    const user = userEvent.setup();
    render(<CertificatesView />);

    await user.click(
      screen.getByRole("button", {
        name: "导出公开 Root CA",
      }),
    );

    await waitFor(() =>
      expect(mocks.certificateExportCa).toHaveBeenCalledTimes(1),
    );
  });

  it("reads fresh SANs before reissuing and refreshes after Rust succeeds", async () => {
    const user = userEvent.setup();
    render(<CertificatesView />);

    await user.click(
      screen.getByRole("button", { name: "重新签发服务端证书" }),
    );

    await waitFor(() =>
      expect(mocks.certificateReissueLeaf).toHaveBeenCalledWith(1, [
        "10.0.28.99",
      ]),
    );
    expect(mocks.overviewRefresh).toHaveBeenCalledTimes(1);
  });

  it("renders the Rust certificate validation result", async () => {
    const user = userEvent.setup();
    render(<CertificatesView />);

    await user.click(screen.getByRole("button", { name: "重新检查" }));

    await waitFor(() =>
      expect(mocks.certificateValidate).toHaveBeenCalledTimes(1),
    );
    expect(screen.getByText("证书即将过期")).toBeVisible();
  });

  it("confirms reset, holds the dialog during Rust work, then refreshes", async () => {
    let finish!: (value: unknown) => void;
    mocks.certificateResetCa.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<CertificatesView />);

    await user.click(
      screen.getByRole("button", { name: "重置本机证书" }),
    );
    await user.click(screen.getByRole("button", { name: "确认重置" }));

    expect(mocks.certificateResetCa).toHaveBeenCalledWith(1, true);
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "正在重置…" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(
      screen.getByRole("alertdialog", { name: "确认重置本机证书？" }),
    ).toBeVisible();

    finish({
      revision: 2,
      ready: true,
      status_text: "本机证书已重置",
      ui_tone: "positive",
      items: [],
      can_initialize: false,
      can_change: true,
      disabled_reason: null,
    });
    await waitFor(() => expect(mocks.overviewRefresh).toHaveBeenCalled());
    await waitFor(() =>
      expect(
        screen.queryByRole("alertdialog", {
          name: "确认重置本机证书？",
        }),
      ).not.toBeInTheDocument(),
    );
  });

  it("keeps the certificate page focused on local Root CA and leaf material", async () => {
    const user = userEvent.setup();
    render(<CertificatesView />);

    expect(screen.getByTestId("certificate-overview-grid")).toHaveClass(
      "grid-cols-2",
      "max-[960px]:grid-cols-1",
    );
    expect(screen.queryByText("导入 / 替换 PKCS12")).not.toBeInTheDocument();
    expect(screen.queryByText("选择性替换上游 CA")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "去配置代理入口" }));
    expect(mocks.navigate).toHaveBeenCalledWith("/listeners");
  });

  it("shows the Root CA and local server leaf public metadata", () => {
    render(<CertificatesView />);

    expect(
      screen.getAllByText("CN=Intercept Proxy Root CA"),
    ).not.toHaveLength(0);
    expect(screen.getAllByText("CN=10.0.34.50")).not.toHaveLength(0);
    expect(screen.getByText("IP:10.0.34.50、DNS:proxy.test")).toBeVisible();
    expect(screen.getByText("AA:BB:CC:DD")).toBeVisible();
    expect(screen.getByText("11:22:33:44")).toBeVisible();
  });

  it("explains that resetting certificates changes the Root CA", () => {
    render(<CertificatesView />);

    expect(
      screen.getByText(/将生成新的本机 Root CA/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/客户端必须删除旧 Root 并导入新 Root/),
    ).toBeInTheDocument();
  });
});
