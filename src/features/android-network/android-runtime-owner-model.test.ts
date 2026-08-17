import { describe, expect, it } from "vitest";
import type {
  AndroidRuntimeOwnerMode,
  AndroidRuntimeOwnerState,
  AndroidRuntimeOwnerTransitionReason,
} from "@/generated/rust-types";
import {
  isForeignRuntimeOwner,
  runtimeOwnerModeText,
  runtimeOwnerQueryKey,
  runtimeOwnerStateText,
  runtimeOwnerTransitionText,
  type RuntimeOwnerDisplay,
} from "./android-runtime-owner-model";

const owner = {
  serial: "device-a",
  mode: "adb_reverse",
  state: "active",
  transition_reason: "activation_confirmed",
} satisfies RuntimeOwnerDisplay;

describe("Android runtime owner model", () => {
  it("keys runtime requests by owner serial and epoch only", () => {
    expect(runtimeOwnerQueryKey({
      serial: "device-a",
      epoch: "11111111-1111-4111-8111-111111111111",
    })).toBe("owner:device-a:11111111-1111-4111-8111-111111111111");
    expect(runtimeOwnerQueryKey(null)).toBeUndefined();
  });

  it("identifies only a different owner as foreign", () => {
    expect(isForeignRuntimeOwner("device-b", "device-a")).toBe(true);
    expect(isForeignRuntimeOwner("device-a", "device-a")).toBe(false);
    expect(isForeignRuntimeOwner(null, null)).toBe(false);
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
