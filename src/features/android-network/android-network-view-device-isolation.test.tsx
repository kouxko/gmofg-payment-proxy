// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AndroidNetworkView } from "./android-network-view";

const mocks = vi.hoisted(() => ({
  androidAdbGet: vi.fn(),
  androidDeviceList: vi.fn(),
  androidAdbSelect: vi.fn(),
  androidPackageList: vi.fn(),
  androidPackageQuery: vi.fn(),
  deviceNetworkProfileList: vi.fn(),
  deviceNetworkProfileNew: vi.fn(),
  deviceNetworkProfileGet: vi.fn(),
  deviceNetworkRuntimeOwners: vi.fn(),
  deviceNetworkEndpoints: vi.fn(),
  workspaceList: vi.fn(),
  workspaceGet: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
vi.mock("@/features/shell/bootstrap-context", () => ({
  useAppEventRefresh: vi.fn(),
}));

function ok<T>(data: T) {
  return Promise.resolve({ status: "ok" as const, data });
}

const profile = {
  id: "profile-1",
  name: "移动网络丢包",
  target_applications: [],
  destination_targets: [],
  proxy_routes: [],
  confirmed_shared_uids: [],
  auto_resume_after_reboot: false,
  weak_network: {
    seed: 1,
    fixed_delay_millis: 0,
    uniform_jitter_millis: 0,
    upload_bytes_per_second: null,
    download_bytes_per_second: null,
    random_loss_basis_points: 0,
    burst_loss: null,
    duplicate_basis_points: 0,
    reorder_basis_points: 0,
    maximum_reorder_hold_millis: 0,
    blackout_windows: [],
    dns_blackhole: false,
    nth_tcp_flag_drops: [],
    path_mtu: { mtu: null, mss_clamp: null, mode: "pass" },
    corruption: { probability_basis_points: 0, bits_per_packet: 0 },
  },
};

describe("Android device UI isolation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: "device-1",
    }));
    mocks.androidDeviceList.mockReturnValue(ok([
      { serial: "device-1", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: true },
      { serial: "device-2", state: "device", product: null, model: "备用设备", device: null, transport_id: "2", selected: false },
    ]));
    mocks.androidPackageList.mockReturnValue(ok([]));
    mocks.androidPackageQuery.mockReturnValue(ok([]));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([]));
    mocks.deviceNetworkProfileNew.mockReturnValue(ok(profile));
    mocks.deviceNetworkRuntimeOwners.mockReturnValue(ok([]));
    mocks.deviceNetworkEndpoints.mockReturnValue(ok({
      configured_profile_id: null,
      configured: [],
      runtime_owner: null,
      runtime: [],
    }));
    mocks.workspaceList.mockReturnValue(ok([{
      id: "workspace-1",
      name: "当前工作区",
      revision: 1,
      listener_count: 0,
      enabled_listener_count: 0,
      selected: true,
    }]));
    mocks.workspaceGet.mockReturnValue(ok({
      id: "workspace-1",
      name: "当前工作区",
      revision: 1,
      listeners: [],
      metadata_extractors: [],
      response_assertions: [],
      fault_presets: [],
      certificate_references: [],
      android_network_profiles: [],
    }));
  });

  it("does not let a late package response from A overwrite selected device B", async () => {
    const user = userEvent.setup();
    let resolveA: ((value: {
      status: "ok";
      data: Array<{ package_name: string; uid: number; shared_uid: null }>;
    }) => void) | undefined;
    mocks.androidPackageList.mockImplementation((serial) => {
      if (serial === "device-1") {
        return new Promise((resolve) => { resolveA = resolve; });
      }
      return ok([{ package_name: "device.b.target", uid: 10002, shared_uid: null }]);
    });

    render(<AndroidNetworkView />);
    await user.click(await screen.findByLabelText("目标设备"));
    await user.click(await screen.findByRole("option", { name: /备用设备/ }));
    await user.click(await screen.findByRole("button", { name: "新建设备网络方案" }));
    expect(await screen.findByText("device.b.target")).toBeVisible();

    await act(async () => resolveA?.(await ok([{
      package_name: "device.a.late",
      uid: 10001,
      shared_uid: null,
    }])));
    expect(screen.queryByText("device.a.late")).not.toBeInTheDocument();
    expect(screen.getByText("device.b.target")).toBeVisible();
  });

  it("keeps late profile reads in the device partition that initiated them", async () => {
    const user = userEvent.setup();
    let resolveA: ((value: Awaited<ReturnType<typeof ok>>) => void) | undefined;
    mocks.deviceNetworkProfileList.mockReturnValue(ok([
      { id: "profile-a", name: "设备 A 方案", target_count: 0, auto_resume_after_reboot: false },
      { id: "profile-b", name: "设备 B 方案", target_count: 0, auto_resume_after_reboot: false },
    ]));
    mocks.deviceNetworkProfileGet.mockImplementation((profileId) => {
      if (profileId === "profile-a") {
        return new Promise((resolve) => { resolveA = resolve; });
      }
      return ok({ ...profile, id: "profile-b", name: "设备 B 方案" });
    });

    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: /设备 A 方案/ }));
    await user.click(screen.getByLabelText("目标设备"));
    await user.click(await screen.findByRole("option", { name: /备用设备/ }));
    await user.click(await screen.findByRole("button", { name: /设备 B 方案/ }));
    expect(await screen.findByDisplayValue("设备 B 方案")).toBeVisible();

    await act(async () => resolveA?.(await ok({
      ...profile,
      id: "profile-a",
      name: "设备 A 方案",
    })));
    expect(screen.getByDisplayValue("设备 B 方案")).toBeVisible();
    expect(screen.queryByDisplayValue("设备 A 方案")).not.toBeInTheDocument();
  });
});
