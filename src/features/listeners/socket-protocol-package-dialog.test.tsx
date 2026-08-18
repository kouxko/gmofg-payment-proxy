import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SocketProtocolPackageDialog } from "./socket-protocol-package-dialog";

const mocks = vi.hoisted(() => ({ protocolPackageDetail: vi.fn() }));

vi.mock("@/generated/rust-types", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/generated/rust-types")>();
  return { ...actual, commands: { ...actual.commands, protocolPackageDetail: mocks.protocolPackageDetail } };
});

vi.mock("@/lib/ipc/client", () => ({
  callCommand: async <T,>(value: Promise<T> | T) => value,
  errorMessage: (reason: unknown) => reason instanceof Error ? reason.message : String(reason),
}));

function detail() {
  return {
    version: {
      package: { id: "iso-8583", version: "2.0.0" },
      name: "ISO 8583 v2",
      host_api: 1,
      built_in: false,
      enabled: true,
      validation: { state: "valid" as const },
      installed_at: "2026-08-15T00:00:00Z",
    },
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: true },
      display: true,
    },
    schema: {
      id: "iso-message",
      version: 7,
      title: "ISO Message",
      fields: [{ name: "mti", label: "MTI", type: "string" as const }],
    },
    usages: [],
  };
}

describe("SocketProtocolPackageDialog", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.protocolPackageDetail.mockResolvedValue(detail());
  });

  it("opens from the keyboard and requests the exact bound package version", async () => {
    const user = userEvent.setup();
    render(<SocketProtocolPackageDialog packageRef={{ id: "iso-8583", version: "2.0.0" }} />);
    const trigger = screen.getByRole("button", { name: "查看所选版本与 Schema" });

    trigger.focus();
    await user.keyboard("{Enter}");

    expect(await screen.findByRole("dialog", { name: "Listener 协议包详情" })).toBeVisible();
    expect(screen.getByText("mti")).toBeVisible();
    expect(mocks.protocolPackageDetail).toHaveBeenCalledWith({ id: "iso-8583", version: "2.0.0" });
  });

  it("closes with Escape and restores focus to the detail trigger", async () => {
    const user = userEvent.setup();
    render(<SocketProtocolPackageDialog packageRef={{ id: "iso-8583", version: "2.0.0" }} />);
    const trigger = screen.getByRole("button", { name: "查看所选版本与 Schema" });
    await user.click(trigger);
    expect(await screen.findByRole("dialog", { name: "Listener 协议包详情" })).toBeVisible();

    await user.keyboard("{Escape}");

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Listener 协议包详情" })).not.toBeInTheDocument());
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("shows exact detail errors inside the dialog", async () => {
    mocks.protocolPackageDetail.mockRejectedValue(new Error("精确版本详情不可用"));
    const user = userEvent.setup();
    render(<SocketProtocolPackageDialog packageRef={{ id: "iso-8583", version: "2.0.0" }} />);

    await user.click(screen.getByRole("button", { name: "查看所选版本与 Schema" }));

    expect(await screen.findByText("协议包详情读取失败")).toBeVisible();
    expect(screen.getByText("精确版本详情不可用")).toBeVisible();
  });

  it("fails closed when the returned detail identity differs from the bound package", async () => {
    mocks.protocolPackageDetail.mockResolvedValue({
      ...detail(),
      version: { ...detail().version, package: { id: "iso-8583", version: "3.0.0" } },
    });
    const user = userEvent.setup();
    render(<SocketProtocolPackageDialog packageRef={{ id: "iso-8583", version: "2.0.0" }} />);

    await user.click(screen.getByRole("button", { name: "查看所选版本与 Schema" }));

    expect(await screen.findByText("协议包详情身份与当前选择不一致。")).toBeVisible();
  });

  it("opens through the real Modal trigger callback without scheduling focus restoration", async () => {
    render(<SocketProtocolPackageDialog packageRef={{ id: "iso-8583", version: "2.0.0" }} />);

    fireEvent.click(screen.getByRole("button", { name: "打开 Listener 协议包详情", hidden: true }));

    expect(await screen.findByRole("dialog", { name: "Listener 协议包详情" })).toBeVisible();
  });

  it("disables the detail trigger while the editor is locked", () => {
    render(<SocketProtocolPackageDialog packageRef={{ id: "iso-8583", version: "2.0.0" }} disabled />);

    expect(screen.getByRole("button", { name: "查看所选版本与 Schema" })).toBeDisabled();
    expect(mocks.protocolPackageDetail).not.toHaveBeenCalled();
  });
});
