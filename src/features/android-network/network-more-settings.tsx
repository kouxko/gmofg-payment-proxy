import type { ReactElement } from "react";
import { Accordion, Card, Chip } from "@heroui/react";
import type {
  AndroidNetworkEndpointSnapshotViewModel,
  AndroidNetworkProfile,
  AndroidProfileEditIntent,
  ProxyListener,
  WeakNetworkProfile,
} from "@/generated/rust-types";
import { AdvancedNetworkCard } from "./advanced-network-card";
import {
  DestinationTargetsCard,
  ExpertNetworkParametersCard,
} from "./network-parameter-cards";
import { ProxyRoutesCard } from "./proxy-routes-card";
import { ProfileRuntimeBehaviorCard } from "./profile-cards";
import { RuntimeEndpointsCard } from "./runtime-endpoints-card";
import type { UpdateWeakNetwork } from "./android-network-types";

interface NetworkMoreSettingsProps {
  draft: AndroidNetworkProfile;
  listeners: ProxyListener[];
  listenersLoading: boolean;
  listenersError?: string;
  endpointsSnapshot?: AndroidNetworkEndpointSnapshotViewModel;
  endpointsLoading: boolean;
  endpointsError?: string;
  onChange: (draft: AndroidNetworkProfile) => void;
  onUpdateWeak: UpdateWeakNetwork;
  onApplyIntent: (intent: AndroidProfileEditIntent) => void;
}

export function NetworkMoreSettings({
  draft,
  listeners,
  listenersLoading,
  listenersError,
  endpointsSnapshot,
  endpointsLoading,
  endpointsError,
  onChange,
  onUpdateWeak,
  onApplyIntent,
}: NetworkMoreSettingsProps): ReactElement {
  const targetCount = draft.destination_targets.length;
  const routeCount = draft.proxy_routes.length;
  const expertCount = configuredExpertEffectCount(draft.weak_network);

  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>更多设置</Card.Title>
        <Card.Description>
          仅在需要调整运行保护、限制作用范围、接入代理调试或复现底层网络故障时展开。
        </Card.Description>
      </Card.Header>
      <Card.Content className="p-4 pt-0">
        <Accordion aria-label="弱网更多设置">
          <SettingsItem
            id="weak-network-runtime-behavior"
            title="运行保护"
            summary={runtimeBehaviorSummary(draft)}
            configured={draft.auto_resume_after_reboot || !draft.stop_vpn_on_control_loss}
          >
            <ProfileRuntimeBehaviorCard draft={draft} onChange={onChange} />
          </SettingsItem>
          <SettingsItem
            id="weak-network-targets"
            title="限制弱网范围"
            summary={targetCount === 0 ? "全部地址" : `${targetCount} 个地址`}
            configured={targetCount > 0}
          >
            <DestinationTargetsCard draft={draft} onChange={onChange} />
          </SettingsItem>
          <SettingsItem
            id="weak-network-proxy"
            title="同时接入代理调试"
            summary={routeCount === 0 ? "未启用，弱网独立运行" : `${routeCount} 条路由`}
            configured={routeCount > 0}
          >
            <div className="space-y-4">
              <ProxyRoutesCard
                draft={draft}
                listeners={listeners}
                loading={listenersLoading}
                error={listenersError}
                onChange={onChange}
              />
              <RuntimeEndpointsCard
                snapshot={endpointsSnapshot}
                loading={endpointsLoading}
                error={endpointsError}
              />
            </div>
          </SettingsItem>
          <SettingsItem
            id="weak-network-expert"
            title="专家参数"
            summary={expertCount === 0 ? "未配置" : `${expertCount} 项已配置`}
            configured={expertCount > 0}
          >
            <div className="space-y-4">
              <ExpertNetworkParametersCard weak={draft.weak_network} onUpdate={onUpdateWeak} />
              <AdvancedNetworkCard
                weak={draft.weak_network}
                onUpdate={onUpdateWeak}
                onApplyIntent={onApplyIntent}
              />
            </div>
          </SettingsItem>
        </Accordion>
      </Card.Content>
    </Card>
  );
}

function SettingsItem({
  id,
  title,
  summary,
  configured,
  children,
}: {
  id: string;
  title: string;
  summary: string;
  configured: boolean;
  children: ReactElement;
}): ReactElement {
  return (
    <Accordion.Item id={id}>
      <Accordion.Heading>
        <Accordion.Trigger>
          <span className="flex min-w-0 flex-1 items-center justify-between gap-3 text-left">
            <span>{title}</span>
            <Chip size="sm" color={configured ? "accent" : "default"} variant="soft">
              {summary}
            </Chip>
          </span>
          <Accordion.Indicator />
        </Accordion.Trigger>
      </Accordion.Heading>
      <Accordion.Panel>
        <Accordion.Body className="pb-4">{children}</Accordion.Body>
      </Accordion.Panel>
    </Accordion.Item>
  );
}

function configuredExpertEffectCount(weak: WeakNetworkProfile): number {
  return [
    weak.seed !== 1,
    weak.duplicate_basis_points > 0,
    weak.reorder_basis_points > 0 || weak.maximum_reorder_hold_millis > 0,
    weak.dns_blackhole,
    weak.burst_loss !== null,
    weak.blackout_windows.length > 0,
    weak.nth_tcp_flag_drops.length > 0,
    weak.path_mtu.mtu !== null || weak.path_mtu.mss_clamp !== null || weak.path_mtu.mode !== "pass",
    weak.corruption.probability_basis_points > 0 || weak.corruption.bits_per_packet > 0,
  ].filter(Boolean).length;
}

function runtimeBehaviorSummary(draft: AndroidNetworkProfile): string {
  if (draft.auto_resume_after_reboot && draft.stop_vpn_on_control_loss) {
    return "自动恢复 · 断联保护";
  }
  if (draft.auto_resume_after_reboot) return "自动恢复";
  if (draft.stop_vpn_on_control_loss) return "断联保护";
  return "已关闭";
}
