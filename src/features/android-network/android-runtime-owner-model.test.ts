import { describe, expect, it } from "vitest";
import type {
  AndroidRuntimeOwnerMode,
  AndroidRuntimeOwnerState,
  AndroidRuntimeOwnerTransitionReason,
} from "@/generated/rust-types";
import {
  runtimeOwnerModeText,
  runtimeOwnerQueryKey,
  clearOwnerConditionally,
  mergeAndroidDeviceTargets,
  runtimeResponseMatches,
  runtimeOwnerStateText,
  runtimeOwnerTransitionText,
} from "./android-runtime-owner-model";

const owner = {
  serial: "device-a",
  epoch: "11111111-1111-4111-8111-111111111111",
  mode: "adb_reverse",
  profile_id: "profile-a",
  state: "active",
  source: "start",
  transition_reason: "activation_confirmed",
  updated_at: "2026-08-17T00:00:00Z",
} as const;

describe("Android runtime owner model", () => {
  it("unions online devices and retained owners by serial", () => {
    const targets = mergeAndroidDeviceTargets([
      { serial: "device-b", state: "device", product: null, model: "B", device: null, transport_id: "2", selected: true },
    ], [
      { ...owner, serial: "device-a" },
      { ...owner, serial: "device-b" },
    ]);

    expect(targets.map((target) => [target.serial, target.online, Boolean(target.owner)])).toEqual([
      ["device-a", false, true],
      ["device-b", true, true],
    ]);
  });

  it("rejects a late response from another serial or an old epoch", () => {
    const target = { serial: "device-a", epoch: owner.epoch };
    expect(runtimeResponseMatches(target, {
      serial: "device-a",
      runtime_epoch: owner.epoch,
    })).toBe(true);
    expect(runtimeResponseMatches(target, {
      serial: "device-b",
      runtime_epoch: owner.epoch,
    })).toBe(false);
    expect(runtimeResponseMatches(target, {
      serial: "device-a",
      runtime_epoch: "22222222-2222-4222-8222-222222222222",
    })).toBe(false);
  });

  it("clears only the exact serial and epoch captured by a completed action", () => {
    const owners = [
      { ...owner },
      { ...owner, serial: "device-b", epoch: "22222222-2222-4222-8222-222222222222" },
    ];
    expect(clearOwnerConditionally(owners, {
      serial: "device-a",
      epoch: "stale-epoch",
    })).toEqual(owners);
    expect(clearOwnerConditionally(owners, owner)).toEqual([owners[1]]);
  });

  it("keys runtime requests by owner serial and epoch only", () => {
    expect(runtimeOwnerQueryKey({
      serial: "device-a",
      epoch: "11111111-1111-4111-8111-111111111111",
    })).toBe("owner:device-a:11111111-1111-4111-8111-111111111111");
    expect(runtimeOwnerQueryKey(null)).toBeUndefined();
  });

  it.each([
    ["device_only", "仅设备端"],
    ["lan", "局域网"],
    ["adb_reverse", "USB / ADB Reverse"],
  ] as const)("maps %s owner mode", (mode, text) => {
    expect(runtimeOwnerModeText(mode)).toBe(text);
  });

  it.each([
    ["active", "运行记录有效"],
    ["uncertain", "运行状态待确认，仍保留运行所有权"],
    ["waiting_reconnect", "设备已断开，等待同一序列号 device-a 重连；仍可停止或紧急恢复"],
    ["cleanup_required", "遗留连接需要清理；请让同一序列号 device-a 重连后停止或紧急恢复"],
    ["stop_failed", "上次停止失败，仍需停止或紧急恢复"],
    ["faulted", "LAN 实际运行端点故障，仍保留运行所有权"],
  ] as const)("maps %s owner state", (state, text) => {
    expect(runtimeOwnerStateText({ ...owner, state })).toBe(text);
  });

  it("fails closed to reconnect copy when a disconnected reason arrives with active state", () => {
    expect(runtimeOwnerStateText({
      ...owner,
      transition_reason: "device_disconnected",
    })).toContain("等待同一序列号 device-a 重连");
  });

  it("fails closed to cleanup copy when cleanup reason arrives with uncertain state", () => {
    expect(runtimeOwnerStateText({
      ...owner,
      state: "uncertain",
      transition_reason: "reverse_cleanup_required",
    })).toContain("遗留连接需要清理");
  });

  it.each([
    ["activation_confirmed", "启动已确认"],
    ["activation_uncertain", "启动结果待确认"],
    ["reverse_preparation", "ADB Reverse 准备中"],
    ["reverse_cleanup_required", "ADB Reverse 仍需清理"],
    ["device_disconnected", "设备已断开"],
    ["device_reconnected", "同序列号设备已重连"],
    ["stop_failed", "停止失败"],
    ["recovered_from_storage", "从本机恢复运行记录"],
    ["lan_endpoint_reapplied", "LAN 实际运行端点已重新应用"],
    ["lan_endpoint_faulted", "LAN 实际运行端点恢复失败"],
  ] as const)("maps %s owner transition", (reason, text) => {
    expect(runtimeOwnerTransitionText(reason)).toBe(text);
  });

  it("fails closed for a runtime owner mode outside the generated union", () => {
    expect(() => runtimeOwnerModeText("future_mode" as AndroidRuntimeOwnerMode))
      .toThrow("未处理的 Android runtime owner 状态");
  });

  it("fails closed for a runtime owner state outside the generated union", () => {
    expect(() => runtimeOwnerStateText({
      ...owner,
      state: "future_state" as AndroidRuntimeOwnerState,
    })).toThrow("未处理的 Android runtime owner 状态");
  });

  it("fails closed for a runtime owner reason outside the generated union", () => {
    expect(() => runtimeOwnerTransitionText(
      "future_reason" as AndroidRuntimeOwnerTransitionReason,
    )).toThrow("未处理的 Android runtime owner 状态");
  });
});
