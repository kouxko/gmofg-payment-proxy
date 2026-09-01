// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProtocolPackagesView } from "./protocol-packages-view";
import { deferred, detail, group, version } from "./protocol-packages-test-support";

const mocks = vi.hoisted(() => ({
  protocolPackageList: vi.fn(),
  protocolPackageDetail: vi.fn(),
  protocolPackageEnable: vi.fn(),
  protocolPackageDisable: vi.fn(),
  protocolPackageRestart: vi.fn(),
  protocolPackageDelete: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : String(reason),
}));
vi.mock("@/features/shell/bootstrap-context", () => ({ useAppEventRefresh: vi.fn() }));

describe("local protocol package lifecycle", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageDisable.mockImplementation(async (packageRef) => version(packageRef.version, {
      package: packageRef,
      package_source: { type: "managed", online: false },
      enabled: false,
    }));
  });

  it("allows an offline local package to enable", async () => {
    const local = version("2.0.0", {
      package_source: { type: "managed", online: false },
      enabled: false,
    });
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [local] })]);
    mocks.protocolPackageDetail.mockResolvedValue(detail(local, {
      usages: [],
      external: null,
    }));
    mocks.protocolPackageEnable.mockResolvedValue({ ...local, enabled: true });

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    const enableButton = await screen.findByRole("button", { name: "启用协议包" });
    expect(enableButton).toBeEnabled();
    expect(screen.queryByRole("button", { name: "重启本地软件包" })).not.toBeInTheDocument();
    await user.click(enableButton);

    await waitFor(() => expect(mocks.protocolPackageEnable).toHaveBeenCalledWith(local.package));
  });

  it("restarts once and locks lifecycle controls while pending", async () => {
    const local = version("2.0.0", {
      package_source: { type: "managed", online: true },
      enabled: true,
    });
    const pending = deferred<ReturnType<typeof version>>();
    const localDetail = detail(local, {
      usages: [],
      external: null,
    });
    mocks.protocolPackageList.mockResolvedValue([group({ versions: [local] })]);
    mocks.protocolPackageDetail.mockResolvedValue(localDetail);
    mocks.protocolPackageRestart.mockReturnValue(pending.promise);

    const user = userEvent.setup();
    render(<ProtocolPackagesView />);
    await user.click(await screen.findByRole("button", { name: "查看协议包 ISO 8583" }));
    const restartButton = await screen.findByRole("button", { name: "重启本地软件包" });
    await Promise.all([user.click(restartButton), user.click(restartButton)]);

    expect(mocks.protocolPackageRestart).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "正在重启…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "停用协议包" })).toBeDisabled();
    pending.resolve({ ...local, package_source: { type: "managed", online: true } });
    await waitFor(() => expect(screen.getByRole("button", { name: "重启本地软件包" })).toBeEnabled());
  });
});
