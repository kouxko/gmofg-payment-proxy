// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AndroidNetworkView } from "./android-network-view";

const mocks = vi.hoisted(() => ({
  androidAdbGet: vi.fn(), androidDeviceList: vi.fn(), androidAdbSelect: vi.fn(),
  androidPackageList: vi.fn(), androidPackageQuery: vi.fn(), deviceNetworkProfileList: vi.fn(), deviceNetworkStatus: vi.fn(),
  deviceNetworkProfileNew: vi.fn(), deviceNetworkProfileGet: vi.fn(), deviceNetworkProfileApplyIntent: vi.fn(), deviceNetworkProfileSave: vi.fn(),
  androidCompanionInstall: vi.fn(), androidCompanionUpdate: vi.fn(), androidVpnOpenConsent: vi.fn(),
  deviceNetworkStart: vi.fn(), deviceNetworkApply: vi.fn(), deviceNetworkStop: vi.fn(),
  deviceNetworkEmergencyRestore: vi.fn(),
}));

vi.mock("@/generated/rust-types", () => ({ commands: mocks }));
function ok<T>(data: T) { return Promise.resolve({ status: "ok" as const, data }); }

const profile = {
  id: "profile-1", name: "移动网络丢包", target_applications: [], destination_targets: [], confirmed_shared_uids: [], auto_resume_after_reboot: false,
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

describe("Android targeted network page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.androidAdbGet.mockReturnValue(ok({ available: true, executable: "/sdk/adb", version: "adb", selected_serial: "device-1" }));
    mocks.androidDeviceList.mockReturnValue(ok([{ serial: "device-1", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: true }]));
    mocks.androidPackageList.mockReturnValue(ok([{ package_name: "example.target", uid: 10001, signing_sha256: "AA", shared_uid: null }]));
    mocks.androidPackageQuery.mockReturnValue(ok([{ package_name: "example.target", uid: 10001, signing_sha256: "AA", shared_uid: null }]));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([]));
    mocks.deviceNetworkStatus.mockReturnValue(ok({ serial: "device-1", state: "stopped", state_text: "已停止", ui_tone: "neutral", verified: true, transport: "local_abstract_socket", active_profile_id: null, companion_process_running: true, message: "已停止", unsupported_fields: [], stats: null }));
    mocks.deviceNetworkProfileNew.mockReturnValue(ok(profile));
    mocks.deviceNetworkProfileApplyIntent.mockImplementation((value, intent) => {
      if (intent.kind === "toggle_package") {
        return ok({
          ...value,
          target_applications: intent.selected ? [{
            package_name: intent.package_name,
            uid: 10001,
            signing_sha256: "AA",
            display_name: intent.package_name,
          }] : [],
        });
      }
      if (intent.kind === "set_burst_loss_enabled") {
        return ok({
          ...value,
          weak_network: {
            ...value.weak_network,
            burst_loss: intent.enabled ? {
              enter_bad_state_basis_points: 0,
              leave_bad_state_basis_points: 0,
              good_state_loss_basis_points: 0,
              bad_state_loss_basis_points: 0,
            } : null,
          },
        });
      }
      if (intent.kind === "add_blackout_window") {
        return ok({ ...value, weak_network: { ...value.weak_network, blackout_windows: [{ start_after_millis: 0, duration_millis: 0 }] } });
      }
      return ok({ ...value, weak_network: { ...value.weak_network, nth_tcp_flag_drops: [{ direction: "upload", flag: "syn", nth: 1 }] } });
    });
    mocks.deviceNetworkProfileSave.mockImplementation((value) => ok(value));
    mocks.deviceNetworkStart.mockReturnValue(ok({ serial: "device-1", state: "running", state_text: "运行中", ui_tone: "positive", verified: true, transport: "local_abstract_socket", active_profile_id: "profile-1", companion_process_running: true, message: "运行中", unsupported_fields: [], stats: null }));
  });

  it("uses compact Chinese labels for the initial operation flow", async () => {
    render(<AndroidNetworkView />);

    expect(await screen.findByRole("heading", { name: "应用定向弱网" })).toBeVisible();
    expect(screen.getByText("设备连接与控制")).toBeVisible();
    expect(screen.getByLabelText("目标设备")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "本机连接工具" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "设备端控制" })).not.toBeInTheDocument();
    expect(screen.getByText("弱网方案")).toBeVisible();
    expect(screen.getByRole("button", { name: "新建弱网方案" })).toBeVisible();
    expect(screen.queryByText("Profiles")).not.toBeInTheDocument();
    expect(screen.queryByText("Companion 与 VPN")).not.toBeInTheDocument();
  });

  it("selects the target device from a dropdown and delegates persistence to Rust", async () => {
    const user = userEvent.setup();
    mocks.androidDeviceList.mockReturnValue(ok([
      { serial: "device-1", state: "device", product: null, model: "A920MAX", device: null, transport_id: "1", selected: true },
      { serial: "device-2", state: "device", product: null, model: "备用设备", device: null, transport_id: "2", selected: false },
    ]));

    render(<AndroidNetworkView />);
    await user.click(await screen.findByLabelText("目标设备"));
    await user.click(await screen.findByRole("option", { name: /备用设备/ }));

    await waitFor(() => expect(mocks.androidAdbSelect).toHaveBeenCalledWith("device-2"));
  });

  it("reports discovered devices before one is selected", async () => {
    mocks.androidAdbGet.mockReturnValue(ok({
      available: true,
      executable: "/sdk/adb",
      version: "adb",
      selected_serial: null,
    }));
    mocks.androidDeviceList.mockReturnValue(ok([{
      serial: "127.0.0.1:6555",
      state: "device",
      product: "motion_phone_arm64",
      model: "Phone",
      device: "motion_phone_arm64",
      transport_id: "1",
      selected: false,
    }]));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("已发现 1 台在线设备，请从下拉框选择。")).toBeVisible();
    expect(screen.queryByText("没有检测到在线设备")).not.toBeInTheDocument();
  });

  it("creates a Rust-owned profile and saves selected applications through Rust", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));
    expect(screen.queryByRole("button", { name: "选择" })).not.toBeInTheDocument();
    expect(screen.getByText(/点击应用所在整行即可选择/)).toBeVisible();
    expect(screen.queryByRole("columnheader", { name: "状态" })).not.toBeInTheDocument();
    expect(screen.queryByText("点击整行选择")).not.toBeInTheDocument();
    const packageRow = await screen.findByRole("row", { name: /example\.target/ });
    await user.click(packageRow);
    await waitFor(() => expect(mocks.deviceNetworkProfileApplyIntent).toHaveBeenCalledWith(
      profile,
      { kind: "toggle_package", package_name: "example.target", selected: true },
    ));
    expect(await screen.findByRole("row", { name: "example.target，已选中" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][0].target_applications[0].package_name).toBe("example.target");
  });

  it("keeps auto-resume track inside the clickable HeroUI switch hit area", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));

    const autoResume = screen.getByRole("switch", {
      name: "解锁且网络可用后自动恢复",
    });
    const content = autoResume.closest('[data-slot="switch-content"]');
    const control = content?.querySelector<HTMLElement>(
      '[data-slot="switch-control"]',
    );

    expect(autoResume).not.toBeChecked();
    expect(control).toBeTruthy();
    expect(content).toContainElement(control!);
    // jsdom 不执行 React Aria SwitchButton 的浏览器级 label 默认动作；结构断言负责
    // 保证可视轨道位于该点击区，隐藏 input 的点击则验证受控状态和保存意图。
    await user.click(autoResume);
    await waitFor(() => expect(screen.getByRole("switch", {
      name: "解锁且网络可用后自动恢复",
    })).toBeChecked());

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][0].auto_resume_after_reboot).toBe(true);
  });

  it("sends package-name filters to Rust instead of filtering the inventory in the page", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));
    await user.type(screen.getByRole("textbox", { name: "包名筛选" }), "example.target");
    await user.click(screen.getByRole("button", { name: "筛选" }));

    await waitFor(() => expect(mocks.androidPackageQuery).toHaveBeenCalledWith("example.target"));
    expect(screen.getByText("example.target")).toBeVisible();
  });

  it("uses a single-column card flow while keeping the full package inventory vertically scrollable", async () => {
    const user = userEvent.setup();
    mocks.androidPackageList.mockReturnValue(ok(Array.from({ length: 20 }, (_, index) => ({
      package_name: `example.target.${index}`,
      uid: 10_001 + index,
      signing_sha256: `AA-${index}`,
      shared_uid: null,
    }))));

    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));

    const applicationTable = await screen.findByLabelText("安卓应用列表");
    const scrollContainer = applicationTable.closest('[data-slot="table-scroll-container"]');
    expect(scrollContainer).toHaveClass("h-80", "overflow-y-auto", "overscroll-contain");
    expect(screen.getByText("example.target.19")).toBeInTheDocument();

    const targetApplicationCard = screen.getByText("目标应用").closest('[data-slot="card"]');
    expect(targetApplicationCard).toHaveClass("border", "border-[var(--telemetry-line)]", "shadow-sm");
    expect(targetApplicationCard?.parentElement).toHaveClass("space-y-4", "overflow-auto");
    expect(targetApplicationCard?.parentElement).not.toHaveClass("grid", "grid-cols-[380px_minmax(0,1fr)]");
  });

  it("starts only after Rust saves the selected profile", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));
    await user.click(await screen.findByRole("row", { name: /example\.target/ }));
    await user.click(screen.getByRole("button", { name: "启动" }));
    await waitFor(() => expect(mocks.deviceNetworkStart).toHaveBeenCalledWith("profile-1", false));
    expect(mocks.deviceNetworkProfileSave.mock.invocationCallOrder[0]).toBeLessThan(mocks.deviceNetworkStart.mock.invocationCallOrder[0]);
  });

  it("collects multiple destination addresses and submits them to Rust", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));

    await user.click(screen.getByRole("button", { name: "添加目标地址" }));
    await user.click(screen.getByRole("button", { name: "添加目标地址" }));
    fireEvent.change(screen.getByLabelText("目标地址 1"), { target: { value: "10.0.34.50" } });
    fireEvent.change(screen.getByLabelText("目标地址 2"), { target: { value: "2001:db8::/32" } });
    const firstPort = screen.getByLabelText("目标地址 1 端口", { selector: "input" });
    await user.clear(firstPort);
    await user.type(firstPort, "16127");

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][0].destination_targets).toEqual([
      { cidr: "10.0.34.50", ports: [16127] },
      { cidr: "2001:db8::/32", ports: [] },
    ]);
  });

  it("collects advanced weak-network intent without validating it in the frontend", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(await screen.findByRole("button", { name: "新建" }));

    await user.click(screen.getByRole("switch", { name: "启用连续丢包" }));
    await user.click(screen.getByRole("button", { name: "添加断网窗口" }));
    await user.click(screen.getByRole("button", { name: "添加 TCP 丢弃" }));

    await waitFor(() => expect(mocks.deviceNetworkProfileApplyIntent).toHaveBeenCalledTimes(3));

    const reorderHold = screen.getByLabelText("乱序保持时间（ms）");
    await user.clear(reorderHold);
    await user.type(reorderHold, "250");
    const corruptionBits = screen.getByLabelText("每包翻转位数", { selector: "input" });
    await user.clear(corruptionBits);
    await user.type(corruptionBits, "65");

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() => expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1));
    const savedWeak = mocks.deviceNetworkProfileSave.mock.calls[0][0].weak_network;
    expect(savedWeak.burst_loss).toEqual({
      enter_bad_state_basis_points: 0,
      leave_bad_state_basis_points: 0,
      good_state_loss_basis_points: 0,
      bad_state_loss_basis_points: 0,
    });
    expect(savedWeak.blackout_windows).toEqual([{ start_after_millis: 0, duration_millis: 0 }]);
    expect(savedWeak.nth_tcp_flag_drops).toEqual([{ direction: "upload", flag: "syn", nth: 1 }]);
    expect(savedWeak.maximum_reorder_hold_millis).toBe(250);
    // 65 超过 Rust 当前允许范围；前端仍原样提交，字段错误应由 Rust 返回。
    expect(savedWeak.corruption.bits_per_packet).toBe(65);
  }, 10_000);
});
