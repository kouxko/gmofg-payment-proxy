import type { ReactElement } from "react";
import { Card } from "@heroui/react";
import type {
  AndroidProfileEditIntent,
  WeakNetworkProfile,
} from "@/generated/rust-types";
import {
  BlackoutWindowsSection,
  BurstLossSection,
} from "./advanced-loss-sections";
import {
  CorruptionSection,
  PathMtuSection,
  TcpFlagDropsSection,
} from "./advanced-transport-sections";
import type { UpdateWeakNetwork } from "./android-network-types";

interface AdvancedNetworkCardProps {
  weak: WeakNetworkProfile;
  onUpdate: UpdateWeakNetwork;
  onApplyIntent: (intent: AndroidProfileEditIntent) => void;
}

export function AdvancedNetworkCard({
  weak,
  onUpdate,
  onApplyIntent,
}: AdvancedNetworkCardProps): ReactElement {
  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>高级 TCP/IP 故障</Card.Title>
        <Card.Description>
          桌面端只记录配置；保存时会统一校验范围、组合关系和危险等级。
        </Card.Description>
      </Card.Header>
      <Card.Content className="space-y-6 p-4">
        <BurstLossSection
          enabled={weak.burst_loss !== null}
          burstLoss={weak.burst_loss}
          onUpdate={onUpdate}
          onEnabledChange={(enabled) => onApplyIntent({
            kind: "set_burst_loss_enabled",
            enabled,
          })}
        />
        <BlackoutWindowsSection
          windows={weak.blackout_windows}
          onUpdate={onUpdate}
          onAdd={() => onApplyIntent({ kind: "add_blackout_window" })}
        />
        <TcpFlagDropsSection
          drops={weak.nth_tcp_flag_drops}
          onUpdate={onUpdate}
          onAdd={() => onApplyIntent({ kind: "add_tcp_flag_drop" })}
        />
        <PathMtuSection pathMtu={weak.path_mtu} onUpdate={onUpdate} />
        <CorruptionSection corruption={weak.corruption} onUpdate={onUpdate} />
      </Card.Content>
    </Card>
  );
}
