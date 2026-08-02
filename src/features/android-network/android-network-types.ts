import type {
  PacketDirection,
  PmtuMode,
  TcpFlag,
  WeakNetworkProfile,
} from "@/generated/rust-types";

export type UpdateWeakNetwork = (changes: Partial<WeakNetworkProfile>) => void;

export const PACKET_DIRECTIONS: Array<{ id: PacketDirection; label: string }> = [
  { id: "upload", label: "上行" },
  { id: "download", label: "下行" },
];

export const TCP_FLAGS: Array<{ id: TcpFlag; label: string }> = [
  { id: "syn", label: "SYN" },
  { id: "syn_ack", label: "SYN-ACK" },
  { id: "ack", label: "ACK" },
  { id: "fin", label: "FIN" },
  { id: "rst", label: "RST" },
];

export const PMTU_MODES: Array<{ id: PmtuMode; label: string }> = [
  { id: "pass", label: "透传" },
  { id: "fragment_or_packet_too_big", label: "IPv4 分片 / IPv6 Packet Too Big" },
  { id: "signal_too_big", label: "返回 Fragmentation Needed / Packet Too Big" },
  { id: "blackhole", label: "PMTU 黑洞" },
];
