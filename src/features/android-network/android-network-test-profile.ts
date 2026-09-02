import type { AndroidNetworkProfile } from "@/generated/rust-types";

export const testAndroidNetworkProfile: AndroidNetworkProfile = {
  id: "profile-1",
  name: "移动网络丢包",
  target_applications: [],
  destination_targets: [],
  proxy_routes: [],
  confirmed_shared_uids: [],
  auto_resume_after_reboot: false,
  stop_vpn_on_control_loss: true,
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
