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
  epoch: "11111111-1111-4111-8111-111111111111",
  mode: "adb_reverse",
  profile_id: "profile-a",
  state: "active",
  source: "start",
  transition_reason: "activation_confirmed",
  updated_at: "2026-08-17T00:00:00Z",
} as const;

function renderDeviceControl(
  selectedSerial: string | null,
  onStop = vi.fn(),
  runtimeOwner: RuntimeOwnerDisplay = ownerA,
  devices = [deviceA, deviceB],
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
      devices={devices}
      devicesLoading={false}
      devicesReady
      devicesError={undefined}
      selectedSerial={selectedSerial}
      runtimeOwners={[{ ...ownerA, ...runtimeOwner }]}
      busySerials={new Set()}
      globalBusy={false}
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
  runtimeOwnerCount = ownerSerial ? 1 : 0,
) {
  render(
    <ProfileActions
      busy={false}
      selectedSerial={selectedSerial}
      runtimeOwner={ownerSerial && ownerSerial === selectedSerial ? { ...ownerA, serial: ownerSerial } : undefined}
      runtimeOwnerCount={runtimeOwnerCount}
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
    expect(screen.getByRole("button", { name: "安装设备端组件" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "刷新运行状态 device-a" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络 device-a" })).toBeEnabled();
  });

  it("keeps owner stop and recovery available without a selected device", async () => {
    const user = userEvent.setup();
    const onStop = vi.fn();
    renderDeviceControl(null, onStop);

    expect(screen.getByLabelText("设备网络运行所有者")).toHaveTextContent("device-a");
    expect(screen.getByRole("button", { name: "安装设备端组件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "更新设备端组件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "刷新运行状态 device-a" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "停止网络接管 device-a" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络 device-a" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "停止网络接管 device-a" }));
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
    expect(screen.getByRole("button", { name: "停止网络接管 device-a" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "紧急恢复网络 device-a" })).toBeEnabled();
  });

  it("keeps an offline owner selectable while disabling ADB-only actions", () => {
    renderDeviceControl("device-a", vi.fn(), ownerA, [deviceB]);

    expect(screen.getByLabelText("目标设备")).toHaveTextContent("离线运行设备 · device-a");
    expect(screen.getByText("离线运行设备：device-a")).toBeVisible();
    expect(screen.getByText(/ADB 安装、更新和授权不可用/)).toBeVisible();
    expect(screen.getByRole("button", { name: "安装设备端组件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "更新设备端组件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "授权网络接管" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "停止网络接管 device-a" })).toBeEnabled();
  });

  it("allows B to start independently from owner A", () => {
    renderProfileActions("device-b", "device-a");

    expect(screen.getByRole("button", { name: "启动" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "停止网络接管" })).not.toBeInTheDocument();
  });

  it("allows the selected owner to apply without duplicating the owner stop action", () => {
    renderProfileActions("device-a", "device-a");

    expect(screen.getByRole("button", { name: "启动" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "停止网络接管" })).not.toBeInTheDocument();
  });

  it("fails closed while the runtime owner is not confirmed", () => {
    renderProfileActions("device-b", null, false);

    expect(screen.getByRole("button", { name: "启动" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.getByText(/确认完成前不能启动或应用方案/)).toBeVisible();
  });

  it("blocks only a new start when the eight-owner capacity is full", () => {
    renderProfileActions("device-b", null, true, 8);

    expect(screen.getByRole("button", { name: "启动" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用修改" })).toBeDisabled();
    expect(screen.getByText(/已达到 8 台运行设备上限/)).toBeVisible();
  });
});
