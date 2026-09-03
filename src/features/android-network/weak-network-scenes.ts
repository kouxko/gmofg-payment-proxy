import type { WeakNetworkProfile } from "@/generated/rust-types";

export type WeakNetworkSceneId = "reference-2g" | "reference-slow-3g" | "reference-slow-4g" | "offline";

export type WeakNetworkCommonSettings = Pick<
  WeakNetworkProfile,
  | "fixed_delay_millis"
  | "uniform_jitter_millis"
  | "upload_bytes_per_second"
  | "download_bytes_per_second"
  | "random_loss_basis_points"
>;

export interface WeakNetworkScene {
  id: WeakNetworkSceneId;
  label: string;
  detail: string;
  sourceLabel: string;
  sourceUrl: string;
  settings: WeakNetworkCommonSettings;
}

/**
 * Published RTT values are halved because this engine applies delay to packets
 * independently in both directions. Published decimal Kbps values are divided
 * by eight to match the existing bytes-per-second profile contract.
 */
export const WEAK_NETWORK_SCENES: readonly WeakNetworkScene[] = [
  {
    id: "reference-2g",
    label: "参考 2G",
    detail: "400 ms RTT · 下行 280 Kbps · 上行 256 Kbps",
    sourceLabel: "sitespeed.io 2g",
    sourceUrl: "https://www.sitespeed.io/documentation/throttle/",
    settings: {
      fixed_delay_millis: 200,
      uniform_jitter_millis: 0,
      upload_bytes_per_second: 32_000,
      download_bytes_per_second: 35_000,
      random_loss_basis_points: 0,
    },
  },
  {
    id: "reference-slow-3g",
    label: "参考慢速 3G",
    detail: "200 ms RTT · 上下行 400 Kbps",
    sourceLabel: "sitespeed.io 3gslow",
    sourceUrl: "https://www.sitespeed.io/documentation/throttle/",
    settings: {
      fixed_delay_millis: 100,
      uniform_jitter_millis: 0,
      upload_bytes_per_second: 50_000,
      download_bytes_per_second: 50_000,
      random_loss_basis_points: 0,
    },
  },
  {
    id: "reference-slow-4g",
    label: "参考慢速 4G",
    detail: "150 ms RTT · 下行 1.6 Mbps · 上行 750 Kbps",
    sourceLabel: "Google Lighthouse mobile preset",
    sourceUrl: "https://github.com/GoogleChrome/lighthouse/blob/main/docs/throttling.md",
    settings: {
      fixed_delay_millis: 75,
      uniform_jitter_millis: 0,
      upload_bytes_per_second: 93_750,
      download_bytes_per_second: 200_000,
      random_loss_basis_points: 0,
    },
  },
  {
    id: "offline",
    label: "完全断网",
    detail: "100% 丢包，用于验证断网与恢复行为",
    sourceLabel: "现有 WeakNetworkProfile 完全丢包合同",
    sourceUrl: "",
    settings: {
      fixed_delay_millis: 0,
      uniform_jitter_millis: 0,
      upload_bytes_per_second: null,
      download_bytes_per_second: null,
      random_loss_basis_points: 10_000,
    },
  },
];

export function matchingWeakNetworkScene(
  weak: WeakNetworkProfile,
): WeakNetworkScene | undefined {
  return WEAK_NETWORK_SCENES.find((scene) => (
    weak.fixed_delay_millis === scene.settings.fixed_delay_millis
    && weak.uniform_jitter_millis === scene.settings.uniform_jitter_millis
    && weak.upload_bytes_per_second === scene.settings.upload_bytes_per_second
    && weak.download_bytes_per_second === scene.settings.download_bytes_per_second
    && weak.random_loss_basis_points === scene.settings.random_loss_basis_points
  ));
}
