import { describe, expect, it } from "vitest";
import { errorMessage } from "./client";

describe("IPC error presentation", () => {
  it("includes Rust field errors instead of hiding the validation cause", () => {
    expect(errorMessage({
      code: "CONFIG_INVALID",
      message: "设备网络方案校验失败",
      field_errors: {
        target_applications: ["必须选择 1 到 64 个目标应用。"],
      },
      retryable: false,
      suggested_action: null,
      entity_id: null,
    })).toBe("设备网络方案校验失败：必须选择 1 到 64 个目标应用。");
  });

  it("deduplicates repeated Rust field messages", () => {
    expect(errorMessage({
      code: "CONFIG_INVALID",
      message: "配置校验失败",
      field_errors: {
        first: ["端口无效。"],
        second: ["端口无效。"],
      },
      retryable: false,
      suggested_action: null,
      entity_id: null,
    })).toBe("配置校验失败：端口无效。");
  });
});
