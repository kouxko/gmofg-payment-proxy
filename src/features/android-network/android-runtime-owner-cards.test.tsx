// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DeviceControlCard } from "./device-control-card";
import { ProfileActions } from "./profile-cards";
import type { RuntimeOwnerDisplay } from "./android-runtime-owner-model";

const deviceA = {
  serial: "device-a",
  state: "device" as const,
  product: null,
  model: "A920MAX",
  device: null,
  transport_id: "1",
  selected: false,
};

const deviceB = {
  ...deviceA,
  serial: "device-b",
  model: "A8700",
  transport_id: "2",
  selected: true,
};

const ownerA = {
  serial: "device-a",
  mode: "adb_reverse",
  state: "active",
  transition_reason: "activation_confirmed",
} satisfies RuntimeOwnerDisplay;

function renderDeviceControl(
  selectedSerial: string | null,
  onStop = vi.fn(),
  runtimeOwner: RuntimeOwnerDisplay = ownerA,
) {
  render(
    <DeviceControlCard
      adb={{
        available: true,
        executable: "/sdk/adb",
        version: "adb",
        selected_serial: selectedSerial,
      }}
      adbLoading={false}
      devices={[deviceA, deviceB]}
      devicesLoading={false}
      selectedSerial={selectedSerial}
      runtimeOwner={runtimeOwner}
      busy={false}
      onRefreshDevices={vi.fn()}
      onSelectDevice={vi.fn()}
      onInstall={vi.fn()}
      onUpdate={vi.fn()}
      onConsent={vi.fn()}
      onRefreshStatus={vi.fn()}
      onStop={onStop}
      onEmergencyRestore={vi.fn()}
    />,
  );
}

function renderProfileActions(
  selectedSerial: string | null,
  ownerSerial: string | null,
  runtimeOwnerReady = true,
) {
  render(
    <ProfileActions
      busy={false}
      selectedSerial={selectedSerial}
      runtimeOwnerSerial={ownerSerial}
      runtimeOwnerReady={runtimeOwnerReady}
      dangerousConfirmed={false}
      onDangerousConfirmedChange={vi.fn()}
      onSave={vi.fn()}
      onStart={vi.fn()}
      onApply={vi.fn()}
    />,
  );
}

describe("Android runtime owner controls", () => {
  it("shows selected B separately from runtime owner A", () => {
    renderDeviceControl("device-b");

    expect(screen.getByLabelText("目标设备")).toHaveTextContent("A8700");
    expect(screen.getByLabelText("设备网络运行所有者")).toHaveTextContent("device-a");
    expect(screen.getByText(/停止、状态查询和紧急恢复只作用于实际运行设备/)).toBeVisible();
    expect(screen.getByRole("button", { name: "安装设备端组件" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "刷新运行状态" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络" })).toBeEnabled();
  });

  it("keeps owner stop and recovery available without a selected device", async () => {
    const user = userEvent.setup();
    const onStop = vi.fn();
    renderDeviceControl(null, onStop);

    expect(screen.getByLabelText("设备网络运行所有者")).toHaveTextContent("device-a");
    expect(screen.getByRole("button", { name: "安装设备端组件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "更新设备端组件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "刷新运行状态" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "停止网络接管" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "停止网络接管" }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it("shows a disconnected owner as waiting for the same serial and keeps recovery actions", () => {
    renderDeviceControl("device-b", vi.fn(), {
      ...ownerA,
      state: "waiting_reconnect",
      transition_reason: "device_disconnected",
    });

    const ownerRegion = screen.getByLabelText("设备网络运行所有者");
    expect(ownerRegion).toHaveTextContent("等待同一序列号 device-a 重连");
    expect(ownerRegion).toHaveTextContent("最近变化：设备已断开");
    expect(ownerRegion).not.toHaveTextContent("运行记录有效");
    expect(screen.getByRole("button", { name: "停止网络接管" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络" })).toBeEnabled();
  });

  it("blocks takeover on B without duplicating the owner stop action", () => {
    renderProfileActions("device-b", "device-a");

    expect(screen.getByRole("button", { name: "启动" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "停止网络接管" })).not.toBeInTheDocument();
    expect(screen.getByText(/请先停止它/)).toBeVisible();
  });

  it("allows the selected owner to apply without duplicating the owner stop action", () => {
    renderProfileActions("device-a", "device-a");

    expect(screen.getByRole("button", { name: "启动" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "停止网络接管" })).not.toBeInTheDocument();
  });

  it("fails closed while the runtime owner is not confirmed", () => {
    renderProfileActions("device-b", null, false);

    expect(screen.getByRole("button", { name: "启动" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.getByText(/确认完成前不能启动或应用方案/)).toBeVisible();
  });
});
