// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  androidNetworkMocks,
  ok,
  overview,
  owner,
  profile,
  setupAndroidNetworkViewMocks,
} from "./android-network-view.test-support";
import { AndroidNetworkView } from "./android-network-view";
const mocks = androidNetworkMocks();
describe("Android targeted network page", () => {
  beforeEach(setupAndroidNetworkViewMocks);
  it("uses compact Chinese labels for the initial operation flow", async () => {
    render(<AndroidNetworkView />);
    expect(await screen.findByRole("heading", { name: "应用网络接管" }))
      .toBeVisible();
    expect(screen.getByText(/即可单独运行弱网/)).toBeVisible();
    expect(screen.queryByText(/填写.*Proxy IP/i)).not.toBeInTheDocument();
    expect(screen.getByText("设备连接与控制")).toBeVisible();
    expect(screen.getByLabelText("目标设备")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "本机连接工具" })).not
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "设备端控制" })).not
      .toBeInTheDocument();
    expect(screen.getByText("设备网络方案")).toBeVisible();
    expect(await screen.findByRole("button", { name: "新建设备网络方案" }))
      .toBeVisible();
    expect(screen.queryByRole("button", { name: "新建" })).not
      .toBeInTheDocument();
    expect(screen.queryByText("尚未选择设备网络方案")).not.toBeInTheDocument();
    expect(screen.queryByText("Profiles")).not.toBeInTheDocument();
    expect(screen.queryByText("Companion 与 VPN")).not.toBeInTheDocument();
  });
  it("keeps one create action and only shows the selection hint when profiles exist", async () => {
    mocks.deviceNetworkProfileList.mockReturnValue(ok([{
      id: "saved-profile",
      name: "已保存方案",
      target_count: 1,
      auto_resume_after_reboot: false,
      stop_vpn_on_control_loss: true,
    }]));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("尚未选择设备网络方案")).toBeVisible();
    expect(screen.getAllByRole("button", { name: "新建" })).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "新建设备网络方案" })).not
      .toBeInTheDocument();
  });
  it("uses a wrapping profile flow and marks the profile currently executed by VPN", async () => {
    mocks.deviceNetworkControlOverview.mockReturnValue(ok(overview([owner()])));
    mocks.deviceNetworkProfileList.mockReturnValue(ok([
      {
        id: "profile-1",
        name: "收银应用弱网",
        target_count: 1,
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
      },
      {
        id: "profile-2",
        name: "扫码应用代理",
        target_count: 2,
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
      },
    ]));
    mocks.deviceNetworkStatus.mockReturnValue(ok({
      serial: "device-1",
      runtime_epoch: "11111111-1111-4111-8111-111111111111",
      state: "running",
      state_text: "运行中",
      ui_tone: "positive",
      verified: true,
      transport: "local_abstract_socket",
      active_profile_id: "profile-1",
      companion_process_running: true,
      message: "设备网络接管正在运行。",
      unsupported_fields: [],
      stats: null,
    }));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("正在执行 · 运行中")).toBeVisible();
    const profileFlow = screen.getByText("收银应用弱网").closest("div.grid");
    expect(profileFlow).toHaveClass("grid", "gap-2");
    expect(screen.getByText("扫码应用代理")).toBeVisible();
  });

  it("keeps showing a running profile that belongs to another Workspace", async () => {
    mocks.deviceNetworkControlOverview.mockReturnValue(ok(overview([owner()])));
    mocks.deviceNetworkStatus.mockReturnValue(ok({
      serial: "device-1",
      runtime_epoch: "11111111-1111-4111-8111-111111111111",
      state: "running",
      state_text: "运行中",
      ui_tone: "positive",
      verified: true,
      transport: "local_abstract_socket",
      active_profile_id: "profile-from-another-workspace",
      companion_process_running: true,
      message: "设备网络接管正在运行。",
      unsupported_fields: [],
      stats: null,
    }));

    render(<AndroidNetworkView />);

    expect(await screen.findByText("其他 Workspace 的方案正在运行"))
      .toBeVisible();
    expect(screen.getByText(/仍使用其原 Workspace 的代理入口/)).toBeVisible();
  });

  it("waits for the previous runtime poll before scheduling the next one", async () => {
    mocks.deviceNetworkControlOverview.mockReturnValue(ok(overview([owner()])));
    vi.useFakeTimers();
    let resolvePoll: (() => void) | undefined;
    mocks.deviceNetworkStatus
      .mockReturnValueOnce(ok({
        serial: "device-1",
        runtime_epoch: "11111111-1111-4111-8111-111111111111",
        state: "stopped",
        state_text: "已停止",
        ui_tone: "neutral",
        verified: true,
        transport: "local_abstract_socket",
        active_profile_id: null,
        companion_process_running: true,
        message: "已停止",
        unsupported_fields: [],
        stats: null,
      }))
      .mockImplementation(() =>
        new Promise((resolve) => {
          resolvePoll = () =>
            resolve({
              status: "ok" as const,
              data: {
                serial: "device-1",
                runtime_epoch: "11111111-1111-4111-8111-111111111111",
                state: "stopped",
                state_text: "已停止",
                ui_tone: "neutral",
                verified: true,
                transport: "local_abstract_socket",
                active_profile_id: null,
                companion_process_running: true,
                message: "已停止",
                unsupported_fields: [],
                stats: null,
              },
            });
        })
      );

    const view = render(<AndroidNetworkView />);
    try {
      await act(async () => undefined);
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(1);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(5_000);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2);

      await act(async () => resolvePoll?.());
      await act(async () => {
        await vi.advanceTimersByTimeAsync(999);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(2);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(mocks.deviceNetworkStatus).toHaveBeenCalledTimes(3);
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  });

  it("selects the target device from a dropdown and delegates persistence to Rust", async () => {
    const user = userEvent.setup();
    mocks.androidDeviceList.mockReturnValue(ok([
      {
        serial: "device-1",
        runtime_epoch: "11111111-1111-4111-8111-111111111111",
        state: "device",
        product: null,
        model: "A920MAX",
        device: null,
        transport_id: "1",
        selected: true,
      },
      {
        serial: "device-2",
        state: "device",
        product: null,
        model: "备用设备",
        device: null,
        transport_id: "2",
        selected: false,
      },
    ]));

    render(<AndroidNetworkView />);
    await user.click(await screen.findByLabelText("目标设备"));
    await user.click(await screen.findByRole("option", { name: /备用设备/ }));

    await waitFor(() =>
      expect(mocks.androidAdbSelect).toHaveBeenCalledWith("device-2")
    );
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

    expect(await screen.findByText("已发现 1 台在线设备，请从下拉框选择。"))
      .toBeVisible();
    expect(screen.queryByText("没有检测到在线设备")).not.toBeInTheDocument();
  });

  it("creates a Rust-owned profile and saves selected applications through Rust", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    expect(screen.queryByRole("button", { name: "选择" })).not
      .toBeInTheDocument();
    expect(screen.getByText(/点击应用所在整行即可选择/)).toBeVisible();
    expect(screen.queryByRole("columnheader", { name: "状态" })).not
      .toBeInTheDocument();
    expect(screen.queryByText("点击整行选择")).not.toBeInTheDocument();
    const packageRow = await screen.findByRole("row", {
      name: /example\.target/,
    });
    await user.click(packageRow);
    await waitFor(() =>
      expect(mocks.deviceNetworkProfileApplyIntent).toHaveBeenCalledWith(
        "device-1",
        profile,
        {
          kind: "toggle_package",
          package_name: "example.target",
          selected: true,
        },
      )
    );
    expect(await screen.findByLabelText("已选中")).toBeVisible();
    expect(screen.getByLabelText("已选择应用")).toBeVisible();
    expect(screen.getByText("已选择应用（1）")).toBeVisible();
    expect(screen.getByLabelText("已选择 example.target")).toBeVisible();
    expect(screen.getByRole("row", { name: /example\.target/ })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() =>
      expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1)
    );
    expect(
      mocks.deviceNetworkProfileSave.mock.calls[0][1].target_applications[0]
        .package_name,
    ).toBe("example.target");
  });

  it("keeps auto-resume track inside the clickable HeroUI switch hit area", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    await user.click(screen.getByRole("button", { name: /运行保护/ }));

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
    await waitFor(() =>
      expect(screen.getByRole("switch", {
        name: "解锁且网络可用后自动恢复",
      })).toBeChecked()
    );

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() =>
      expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1)
    );
    expect(
      mocks.deviceNetworkProfileSave.mock.calls[0][1].auto_resume_after_reboot,
    ).toBe(true);
  });

  it("defaults control-loss protection on and persists the user choice", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    await user.click(screen.getByRole("button", { name: /运行保护/ }));

    const protection = screen.getByRole("switch", {
      name: "ADB 或桌面控制失联 5 秒后自动关闭 VPN",
    });
    expect(protection).toBeChecked();

    await user.click(protection);
    expect(protection).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "保存方案" }));

    await waitFor(() =>
      expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1)
    );
    expect(
      mocks.deviceNetworkProfileSave.mock.calls[0][1].stop_vpn_on_control_loss,
    ).toBe(false);
  });

  it("sends package-name filters to Rust instead of filtering the inventory in the page", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    await user.type(
      screen.getByRole("textbox", { name: "包名筛选" }),
      "example.target",
    );
    await user.click(screen.getByRole("button", { name: "筛选" }));

    await waitFor(() =>
      expect(mocks.androidPackageQuery).toHaveBeenCalledWith(
        "device-1",
        "example.target",
      )
    );
    expect(screen.getByText("example.target")).toBeVisible();
  });

  it("refreshes the Rust package inventory and reapplies the active package filter", async () => {
    const user = userEvent.setup();
    mocks.androidPackageQuery
      .mockReturnValueOnce(ok([]))
      .mockReturnValueOnce(ok([{
        package_name: "com.example.installed",
        uid: 10002,
        shared_uid: null,
      }]));
    mocks.androidPackageRefresh.mockReturnValue(ok([{
      package_name: "com.example.installed",
      uid: 10002,
      shared_uid: null,
    }]));

    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    fireEvent.change(screen.getByRole("textbox", { name: "包名筛选" }), {
      target: { value: "com.example.installed" },
    });
    await user.click(screen.getByRole("button", { name: "筛选" }));
    expect(await screen.findByText("没有匹配该包名的应用。")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "刷新应用列表" }));

    await waitFor(() =>
      expect(mocks.androidPackageRefresh).toHaveBeenCalledTimes(1)
    );
    await waitFor(() =>
      expect(mocks.androidPackageQuery).toHaveBeenCalledTimes(2)
    );
    expect(await screen.findByText("com.example.installed")).toBeVisible();
  });
});
