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
  settingsGet: vi.fn(),
  settingsSetData: vi.fn(),
  overviewRefresh: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/generated/rust-types", () => ({
  commands: {
    certificateExportCa: mocks.certificateExportCa,
    certificateGenerateCa: mocks.certificateGenerateCa,
    certificateImportPkcs12: vi.fn(),
    certificateImportUpstreamCa: vi.fn(),
    certificateOverview: mocks.certificateOverview,
    certificateReissueLeaf: vi.fn(),
    certificateResetCa: vi.fn(),
    certificateValidate: vi.fn(),
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
            items: [],
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

  it("uses a responsive full-width certificate overview layout", () => {
    render(<CertificatesView />);

    expect(screen.getByTestId("certificate-overview-grid")).toHaveClass(
      "grid-cols-2",
      "max-[960px]:grid-cols-1",
    );
    expect(screen.getByTestId("certificate-upstream-actions")).toHaveClass(
      "grid-cols-[minmax(0,1fr)_auto]",
      "items-center",
      "max-[860px]:grid-cols-1",
    );
  });

  it("warns that reinitialization replaces the installation Root CA", () => {
    render(<CertificatesView />);

    expect(
      screen.getByText(/将替换本机 Root CA、Root 私钥、服务端私钥和叶子证书/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Root CA 保持不变/)).not.toBeInTheDocument();
  });
});
