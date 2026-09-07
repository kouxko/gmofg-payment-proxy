// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  androidAdbGet: vi.fn(), androidDeviceList: vi.fn(), androidPackageList: vi.fn(),
  androidPackageQuery: vi.fn(), deviceNetworkProfileList: vi.fn(),
  deviceNetworkControlOverview: vi.fn(), deviceNetworkStatus: vi.fn(),
  deviceNetworkEndpoints: vi.fn(),
}));
vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({ useAppEventRefresh: vi.fn() }));

import { useAndroidNetworkController } from "./use-android-network-controller";

const ok = <T,>(data: T) => Promise.resolve({ status: "ok" as const, data });

describe("useAndroidNetworkController", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.androidAdbGet.mockReturnValue(ok({ selected_serial: "device-1" }));
    mocks.androidDeviceList.mockReturnValue(ok([]));
    mocks.androidPackageList.mockReturnValue(ok([]));
    mocks.androidPackageQuery.mockReturnValue(ok([]));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([]));
    mocks.deviceNetworkControlOverview.mockReturnValue(ok({
      owners: [{ serial: "device-1", epoch: "epoch-1", profile_id: "profile-1" }],
      max_runtime_owners: 8,
      can_start_new_owner: true,
    }));
    mocks.deviceNetworkStatus.mockReturnValue(ok({ serial: "device-1", runtime_epoch: "epoch-1" }));
    mocks.deviceNetworkEndpoints.mockReturnValue(ok({ configured: [], runtime: [] }));
  });
  afterEach(() => vi.useRealTimers());

  it("uses one owned poll loop and stops scheduling refreshes after disposal", async () => {
    const view = renderHook(() => useAndroidNetworkController());
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(mocks.deviceNetworkControlOverview).toHaveBeenCalled();

    const beforePoll = mocks.androidDeviceList.mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    expect(mocks.androidDeviceList.mock.calls.length).toBeGreaterThan(beforePoll);
    const beforeDispose = mocks.androidDeviceList.mock.calls.length;

    view.unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
    expect(mocks.androidDeviceList).toHaveBeenCalledTimes(beforeDispose);
  });
});
