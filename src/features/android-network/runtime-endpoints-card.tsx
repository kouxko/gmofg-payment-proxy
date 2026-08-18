import { Alert, Card, Chip } from "@heroui/react";
import type { ReactNode } from "react";
import type {
  AndroidNetworkEndpointSnapshotViewModel,
  AndroidRuntimeEndpointHealth,
} from "@/generated/rust-types";
import { runtimeOwnerModeText } from "./android-runtime-owner-model";

interface RuntimeEndpointsCardProps {
  snapshot?: AndroidNetworkEndpointSnapshotViewModel;
  loading: boolean;
  error?: string;
}

export function RuntimeEndpointsCard({
  snapshot,
  loading,
  error,
}: RuntimeEndpointsCardProps) {
  return (
    <Card className="border border-[var(--telemetry-line)] shadow-sm">
      <Card.Header>
        <Card.Title>代理端点</Card.Title>
        <Card.Description>
          配置值来自当前方案；实际值始终来自运行所有者及其 epoch。
        </Card.Description>
      </Card.Header>
      <Card.Content className="grid gap-4 p-4 lg:grid-cols-2">
        {error && (
          <Alert status="danger" className="lg:col-span-2">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>端点状态读取失败</Alert.Title>
              <Alert.Description>{error}</Alert.Description>
            </Alert.Content>
          </Alert>
        )}
        <EndpointSection title="方案配置端点" label="方案配置端点">
          {loading && !snapshot ? (
            <EmptyText>正在读取配置端点…</EmptyText>
          ) : snapshot?.configured.length ? (
            snapshot.configured.map((endpoint) => (
              <div key={`${endpoint.listener_id}:${endpoint.original_destination}`} className="space-y-1 rounded-lg bg-[var(--telemetry-soft)] p-3 text-xs">
                <p><strong>目标：</strong>{formatDestination(endpoint.original_destination, endpoint.original_ports)}</p>
                <p><strong>Listener：</strong>{endpoint.listener_name}（{endpoint.listener_id}）</p>
                <p><strong>配置监听：</strong>{formatHostPort(endpoint.listener_bind_address, endpoint.listener_port)}</p>
              </div>
            ))
          ) : (
            <EmptyText>当前方案没有配置桌面代理路由。</EmptyText>
          )}
        </EndpointSection>

        <EndpointSection title="实际运行端点" label="实际运行端点">
          {loading && !snapshot ? (
            <EmptyText>正在读取实际运行端点…</EmptyText>
          ) : snapshot?.runtime.length ? (
            snapshot.runtime.map((endpoint) => (
              <div key={`${endpoint.epoch}:${endpoint.listener_id}:${endpoint.original_destination}`} className="space-y-1 rounded-lg bg-[var(--telemetry-soft)] p-3 text-xs" data-health={endpoint.health}>
                <div className="flex flex-wrap items-center gap-2">
                  <Chip color={healthColor(endpoint.health)} variant="soft" size="sm">
                    {healthText(endpoint.health)}
                  </Chip>
                  <span>{runtimeOwnerModeText(endpoint.mode)}</span>
                </div>
                <p><strong>运行设备：</strong><span className="font-mono">{endpoint.serial}</span></p>
                <p><strong>代理地址：</strong>{formatHostPort(endpoint.proxy_host, endpoint.proxy_port)}</p>
                <p><strong>Listener：</strong>{endpoint.listener_name}（{endpoint.listener_id}）· 桌面端口 {endpoint.desktop_listener_port}</p>
                <p><strong>原始目标：</strong>{formatDestination(endpoint.original_destination, endpoint.original_ports)}</p>
                <p><strong>解析地址：</strong>{endpoint.resolved_original_ips.join("、") || "无"}</p>
                <p><strong>解析时间：</strong><time dateTime={endpoint.resolved_at}>{endpoint.resolved_at}</time></p>
              </div>
            ))
          ) : (
            <EmptyText>当前没有实际运行端点。</EmptyText>
          )}
          {snapshot?.runtime_owner?.transition_reason === "lan_endpoint_reapplied" && (
            <p className="text-xs text-[var(--telemetry-success)]">LAN 地址变化后，实际运行端点已重新应用。</p>
          )}
          {snapshot?.runtime_owner?.transition_reason === "lan_endpoint_faulted" && (
            <p className="text-xs text-[var(--telemetry-danger)]">LAN 地址变化后无法恢复实际运行端点；请检查桌面网络、Listener 与设备可达性。</p>
          )}
        </EndpointSection>
      </Card.Content>
    </Card>
  );
}

function EndpointSection({ title, label, children }: { title: string; label: string; children: ReactNode }) {
  return (
    <section aria-label={label} className="space-y-3 rounded-xl border border-[var(--telemetry-line)] p-3">
      <h3 className="text-sm font-medium">{title}</h3>
      {children}
    </section>
  );
}

function EmptyText({ children }: { children: ReactNode }) {
  return <p className="text-xs text-[var(--telemetry-muted)]">{children}</p>;
}

function formatDestination(destination: string, ports: number[]) {
  return ports.length ? `${destination}:${ports.join(",")}` : destination;
}

function formatHostPort(host: string, port: number) {
  return host.includes(":") && !host.startsWith("[") ? `[${host}]:${port}` : `${host}:${port}`;
}

function healthText(health: AndroidRuntimeEndpointHealth) {
  if (health === "healthy") return "健康";
  if (health === "waiting_reconnect") return "等待重连";
  return "故障";
}

function healthColor(health: AndroidRuntimeEndpointHealth): "success" | "warning" | "danger" {
  if (health === "healthy") return "success";
  if (health === "waiting_reconnect") return "warning";
  return "danger";
}
