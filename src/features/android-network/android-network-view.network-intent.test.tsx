// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import {
  androidNetworkMocks,
  ok,
  setupAndroidNetworkViewMocks,
} from "./android-network-view.test-support";
import { AndroidNetworkView } from "./android-network-view";
const mocks = androidNetworkMocks();

describe("Android targeted network layout and routing intent", () => {
  beforeEach(setupAndroidNetworkViewMocks);
  it("uses a single-column card flow while keeping the full package inventory vertically scrollable", async () => {
    const user = userEvent.setup();
    mocks.androidPackageList.mockReturnValue(
      ok(Array.from({ length: 20 }, (_, index) => ({
        package_name: `example.target.${index}`,
        uid: 10_001 + index,
        shared_uid: null,
      }))),
    );

    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );

    const applicationTable = await screen.findByLabelText("安卓应用列表");
    const scrollContainer = applicationTable.closest(
      '[data-slot="table-scroll-container"]',
    );
    expect(scrollContainer).toHaveClass(
      "h-80",
      "overflow-y-auto",
      "overscroll-contain",
    );
    expect(screen.getByText("example.target.19")).toBeInTheDocument();

    const targetApplicationCard = screen.getByText("目标应用").closest(
      '[data-slot="card"]',
    );
    expect(targetApplicationCard).toHaveClass(
      "border",
      "border-[var(--telemetry-line)]",
      "shadow-sm",
    );
    expect(targetApplicationCard?.parentElement).toHaveClass(
      "space-y-4",
      "overflow-auto",
    );
    expect(targetApplicationCard?.parentElement).not.toHaveClass(
      "grid",
      "grid-cols-[380px_minmax(0,1fr)]",
    );
  });
  it("starts only after Rust saves the selected profile", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    await user.click(
      await screen.findByRole("row", { name: /example\.target/ }),
    );
    await user.click(screen.getByRole("button", { name: "启动" }));
    await waitFor(() =>
      expect(mocks.deviceNetworkStart).toHaveBeenCalledWith(
        "device-1",
        "profile-1",
        false,
      )
    );
    expect(mocks.deviceNetworkProfileSave.mock.invocationCallOrder[0])
      .toBeLessThan(mocks.deviceNetworkStart.mock.invocationCallOrder[0]);
  });

  it("starts the complete-outage scene only with the explicit risk confirmation", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );
    await user.click(
      await screen.findByRole("row", { name: /example\.target/ }),
    );
    await user.click(screen.getByRole("button", { name: "完全断网" }));

    const confirmation = screen.getByRole("switch", {
      name: "确认 100% 丢包或黑洞风险",
    });
    expect(confirmation).toBeVisible();
    await user.click(confirmation);
    await user.click(screen.getByRole("button", { name: "启动" }));

    await waitFor(() =>
      expect(mocks.deviceNetworkStart).toHaveBeenCalledWith(
        "device-1",
        "profile-1",
        true,
      )
    );
    expect(
      mocks.deviceNetworkProfileSave.mock.calls[0][1].weak_network
        .random_loss_basis_points,
    ).toBe(10_000);
  });

  it(
    "collects multiple transparent proxy routes using current Workspace listeners",
    async () => {
      const user = userEvent.setup();
      render(<AndroidNetworkView />);
      await user.click(
        await screen.findByRole("button", { name: "新建设备网络方案" }),
      );

      await user.click(
        screen.getByRole("button", { name: /同时接入代理调试/ }),
      );
      expect(screen.getByText(/业务 App 仍访问原始 Server/)).toBeVisible();
      expect(screen.queryByLabelText(/Proxy IP/i)).not.toBeInTheDocument();
      await user.click(
        screen.getByRole("button", { name: "添加透明代理路由" }),
      );
      await user.click(
        screen.getByRole("button", { name: "添加透明代理路由" }),
      );
      expect(screen.getAllByText(/必须至少添加一个原始端口/)).toHaveLength(2);
      await user.type(screen.getByLabelText("原始目标 1"), "api.example.test");
      await user.type(screen.getByLabelText("原始目标 2"), "10.0.34.50");

      await user.click(screen.getAllByRole("button", { name: "添加端口" })[0]);
      await user.click(screen.getAllByRole("button", { name: "添加端口" })[0]);
      const firstPort = screen.getByLabelText("原始目标 1 端口 1", {
        selector: "input",
      });
      const secondPort = screen.getByLabelText("原始目标 1 端口 2", {
        selector: "input",
      });
      await user.clear(firstPort);
      await user.type(firstPort, "443");
      await user.clear(secondPort);
      await user.type(secondPort, "8443");

      await user.click(screen.getByLabelText("原始目标 2 代理入口"));
      await user.click(await screen.findByRole("option", { name: /DLL 入口/ }));
      await user.click(screen.getByRole("button", { name: "保存方案" }));

      await waitFor(() =>
        expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1)
      );
      expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].proxy_routes)
        .toEqual([
          {
            destination: "api.example.test",
            ports: [443, 8443],
            listener_id: "listener-1",
          },
          { destination: "10.0.34.50", ports: [], listener_id: "listener-2" },
        ]);
    },
    20_000,
  );

  it("collects multiple weak-network coverage addresses without treating them as proxy routes", async () => {
    const user = userEvent.setup();
    render(<AndroidNetworkView />);
    await user.click(
      await screen.findByRole("button", { name: "新建设备网络方案" }),
    );

    await user.click(screen.getByRole("button", { name: /限制弱网范围/ }));
    expect(screen.getByText("弱网覆盖范围（可选）")).toBeVisible();
    expect(screen.getByText(/这里只限制哪些连接实施弱网，不改变请求去向/))
      .toBeVisible();

    await user.click(screen.getByRole("button", { name: "添加弱网覆盖地址" }));
    await user.click(screen.getByRole("button", { name: "添加弱网覆盖地址" }));
    fireEvent.change(screen.getByLabelText("目标地址 1"), {
      target: { value: "10.0.34.50" },
    });
    fireEvent.change(screen.getByLabelText("目标地址 2"), {
      target: { value: "2001:db8::/32" },
    });
    const firstPort = screen.getByLabelText("目标地址 1 端口", {
      selector: "input",
    });
    await user.clear(firstPort);
    await user.type(firstPort, "16127");

    await user.click(screen.getByRole("button", { name: "保存方案" }));
    await waitFor(() =>
      expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1)
    );
    expect(mocks.deviceNetworkProfileSave.mock.calls[0][1].destination_targets)
      .toEqual([
        { cidr: "10.0.34.50", ports: [16127] },
        { cidr: "2001:db8::/32", ports: [] },
      ]);
  });

  it(
    "collects advanced weak-network intent without validating it in the frontend",
    async () => {
      const user = userEvent.setup();
      render(<AndroidNetworkView />);
      await user.click(
        await screen.findByRole("button", { name: "新建设备网络方案" }),
      );

      await user.click(screen.getByRole("button", { name: /专家参数/ }));
      await user.click(screen.getByRole("switch", { name: "启用连续丢包" }));
      await user.click(screen.getByRole("button", { name: "添加断网窗口" }));
      await user.click(screen.getByRole("button", { name: "添加 TCP 丢弃" }));

      await waitFor(() =>
        expect(mocks.deviceNetworkProfileApplyIntent).toHaveBeenCalledTimes(3)
      );

      const reorderHold = screen.getByLabelText("乱序保持时间（ms）");
      await user.clear(reorderHold);
      await user.type(reorderHold, "250");
      const corruptionBits = screen.getByLabelText("每包翻转位数", {
        selector: "input",
      });
      await user.clear(corruptionBits);
      await user.type(corruptionBits, "65");

      await user.click(screen.getByRole("button", { name: "保存方案" }));
      await waitFor(() =>
        expect(mocks.deviceNetworkProfileSave).toHaveBeenCalledTimes(1)
      );
      const savedWeak =
        mocks.deviceNetworkProfileSave.mock.calls[0][1].weak_network;
      expect(savedWeak.burst_loss).toEqual({
        enter_bad_state_basis_points: 0,
        leave_bad_state_basis_points: 0,
        good_state_loss_basis_points: 0,
        bad_state_loss_basis_points: 0,
      });
      expect(savedWeak.blackout_windows).toEqual([{
        start_after_millis: 0,
        duration_millis: 0,
      }]);
      expect(savedWeak.nth_tcp_flag_drops).toEqual([{
        direction: "upload",
        flag: "syn",
        nth: 1,
      }]);
      expect(savedWeak.maximum_reorder_hold_millis).toBe(250);
      // 65 超过 Rust 当前允许范围；前端仍原样提交，字段错误应由 Rust 返回。
      expect(savedWeak.corruption.bits_per_packet).toBe(65);
    },
    10_000,
  );
});
