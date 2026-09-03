// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";
import type { AndroidNetworkProfile, WeakNetworkProfile } from "@/generated/rust-types";
import { BasicNetworkParametersCard } from "./network-parameter-cards";
import { NetworkMoreSettings } from "./network-more-settings";
import { testAndroidNetworkProfile } from "./android-network-test-profile";
import { WEAK_NETWORK_SCENES } from "./weak-network-scenes";

describe("simplified standalone weak-network editor", () => {
  it("keeps optional proxy and expert settings collapsed by default", async () => {
    const user = userEvent.setup();
    render(<TestWeakNetworkEditor />);

    expect(screen.getByText("常用弱网效果")).toBeVisible();
    expect(screen.getByText(/无需配置代理入口/)).toBeVisible();
    expect(screen.getByLabelText("延迟（ms）")).toBeVisible();
    expect(screen.getByLabelText("丢包率（%）")).toBeVisible();
    expect(screen.getByLabelText("随机种子")).not.toBeVisible();
    expect(screen.queryByRole("button", { name: "添加透明代理路由" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /同时接入代理调试/ }));
    expect(screen.getByRole("button", { name: "添加透明代理路由" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: /专家参数/ }));
    expect(screen.getByLabelText("随机种子")).toBeVisible();
  });

  it("shows loss as a percentage while retaining the basis-point contract", async () => {
    const user = userEvent.setup();
    render(<TestWeakNetworkEditor />);

    const packetLoss = screen.getByLabelText("丢包率（%）");
    await user.clear(packetLoss);
    await user.type(packetLoss, "2.5");
    await user.tab();

    await waitFor(() => expect(currentProfile().weak_network.random_loss_basis_points).toBe(250));
    expect(screen.getByRole("button", { name: "自定义" })).toHaveAttribute("aria-pressed", "true");
  });

  it("applies the sourced slow-4G scene to the existing fields", async () => {
    const user = userEvent.setup();
    render(<TestWeakNetworkEditor />);

    await user.click(screen.getByRole("button", { name: "参考慢速 4G" }));

    expect(screen.getByRole("button", { name: "参考慢速 4G" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/来源：Google Lighthouse mobile preset/)).toBeVisible();
    expect(currentProfile().weak_network).toMatchObject({
      fixed_delay_millis: 75,
      uniform_jitter_millis: 0,
      upload_bytes_per_second: 93_750,
      download_bytes_per_second: 200_000,
      random_loss_basis_points: 0,
    });
  });

  it("maps complete outage onto the existing full-loss field", async () => {
    const user = userEvent.setup();
    render(<TestWeakNetworkEditor />);

    await user.click(screen.getByRole("button", { name: "完全断网" }));

    expect(screen.getByLabelText("丢包率（%）")).toHaveValue("100");
    expect(currentProfile().weak_network).toMatchObject({
      fixed_delay_millis: 0,
      upload_bytes_per_second: null,
      download_bytes_per_second: null,
      random_loss_basis_points: 10_000,
    });
  });

  it("keeps every published reference value explicit and reviewable", () => {
    expect(WEAK_NETWORK_SCENES.map(({ id, settings }) => ({ id, ...settings }))).toEqual([
      { id: "reference-2g", fixed_delay_millis: 200, uniform_jitter_millis: 0, upload_bytes_per_second: 32_000, download_bytes_per_second: 35_000, random_loss_basis_points: 0 },
      { id: "reference-slow-3g", fixed_delay_millis: 100, uniform_jitter_millis: 0, upload_bytes_per_second: 50_000, download_bytes_per_second: 50_000, random_loss_basis_points: 0 },
      { id: "reference-slow-4g", fixed_delay_millis: 75, uniform_jitter_millis: 0, upload_bytes_per_second: 93_750, download_bytes_per_second: 200_000, random_loss_basis_points: 0 },
      { id: "offline", fixed_delay_millis: 0, uniform_jitter_millis: 0, upload_bytes_per_second: null, download_bytes_per_second: null, random_loss_basis_points: 10_000 },
    ]);
  });

  it("resynchronizes the selected scene when the same profile receives authoritative parameters", () => {
    const onUpdate = vi.fn();
    const slow4g = WEAK_NETWORK_SCENES.find((scene) => scene.id === "reference-slow-4g")!;
    const slow3g = WEAK_NETWORK_SCENES.find((scene) => scene.id === "reference-slow-3g")!;
    const { rerender } = render(
      <BasicNetworkParametersCard
        weak={{ ...testAndroidNetworkProfile.weak_network, ...slow4g.settings }}
        onUpdate={onUpdate}
      />,
    );

    expect(screen.getByRole("button", { name: "参考慢速 4G" })).toHaveAttribute("aria-pressed", "true");

    rerender(
      <BasicNetworkParametersCard
        weak={{ ...testAndroidNetworkProfile.weak_network, ...slow3g.settings }}
        onUpdate={onUpdate}
      />,
    );

    expect(screen.getByRole("button", { name: "参考慢速 3G" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "参考慢速 4G" })).toHaveAttribute("aria-pressed", "false");
  });
});

function TestWeakNetworkEditor(): ReactElement {
  const [draft, setDraft] = useState<AndroidNetworkProfile>(testAndroidNetworkProfile);
  const updateWeak = (changes: Partial<WeakNetworkProfile>) => setDraft((current) => ({
    ...current,
    weak_network: { ...current.weak_network, ...changes },
  }));

  return (
    <>
      <BasicNetworkParametersCard weak={draft.weak_network} onUpdate={updateWeak} />
      <NetworkMoreSettings
        draft={draft}
        listeners={[]}
        listenersLoading={false}
        endpointsLoading={false}
        onChange={setDraft}
        onUpdateWeak={updateWeak}
        onApplyIntent={() => undefined}
      />
      <output data-testid="current-profile">{JSON.stringify(draft)}</output>
    </>
  );
}

function currentProfile(): AndroidNetworkProfile {
  return JSON.parse(screen.getByTestId("current-profile").textContent ?? "") as AndroidNetworkProfile;
}
