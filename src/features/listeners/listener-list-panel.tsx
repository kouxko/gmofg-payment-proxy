import { Alert, Button, Spinner, Table } from "@heroui/react";
import type { ProxyListener } from "@/generated/rust-types";

export function ListenerListPanel({
  listeners,
  selectedIndex,
  loading,
  error,
  disabled,
  onAdd,
  onSelect,
  onNavigate,
}: {
  listeners: ProxyListener[];
  selectedIndex: number;
  loading: boolean;
  error?: string;
  disabled: boolean;
  onAdd: () => void;
  onSelect: (index: number) => void;
  onNavigate: (path: string) => void;
}) {
  return <aside className="min-w-0 space-y-4 overflow-auto border-r border-[var(--telemetry-line)] p-5 max-[900px]:border-r-0 max-[900px]:border-b">
    <div><h1 className="text-2xl font-semibold">代理监听</h1>
      <p className="mt-1 text-sm text-[var(--telemetry-muted)]">统一管理 HTTP 代理与原始 Socket 双向转发。</p>
    </div>
    <Button variant="primary" className="w-full" isDisabled={disabled} onPress={onAdd}>新建代理监听</Button>
    <Alert status="accent"><Alert.Indicator /><Alert.Content>
      <Alert.Title>HTTP 与 Socket 两种数据平面</Alert.Title>
      <Alert.Description>
        新建监听默认使用 HTTP；Socket Relay 需配置唯一 Server 上游，
        LocalResponder 配置暂时只支持安全读取。
      </Alert.Description>
    </Alert.Content></Alert>
    <Alert status="warning"><Alert.Indicator /><Alert.Content>
      <Alert.Title>故障模拟与规则作用于监听流量</Alert.Title>
      <Alert.Description>启动监听后，到故障模拟或拦截规则页面配置行为。</Alert.Description>
      <div className="mt-3 flex gap-2">
        <Button size="sm" variant="primary" onPress={() => onNavigate("/faults")}>去添加故障模拟</Button>
        <Button size="sm" variant="outline" onPress={() => onNavigate("/rules")}>去配置拦截规则</Button>
      </div>
    </Alert.Content></Alert>
    {loading && <Spinner aria-label="正在读取代理监听" />}
    {error && <Alert status="danger"><Alert.Indicator /><Alert.Content>
      <Alert.Title>读取失败</Alert.Title><Alert.Description>{error}</Alert.Description>
    </Alert.Content></Alert>}
    <ListenerTable listeners={listeners} selectedIndex={selectedIndex} onSelect={onSelect} />
  </aside>;
}

function ListenerTable({ listeners, selectedIndex, onSelect }: {
  listeners: ProxyListener[];
  selectedIndex: number;
  onSelect: (index: number) => void;
}) {
  return <Table><Table.ScrollContainer><Table.Content aria-label="代理监听列表">
    <Table.Header><Table.Column isRowHeader>监听名称</Table.Column><Table.Column>客户端连接 → 请求去向</Table.Column></Table.Header>
    <Table.Body renderEmptyState={() => <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">当前工作区还没有代理监听</div>}>
      {listeners.map((listener, index) => <Table.Row key={listener.id} id={listener.id}
        onAction={() => onSelect(index)} className={index === selectedIndex ? "bg-[var(--telemetry-accent-soft)]" : ""}>
        <Table.Cell><span className="font-medium">{listener.name}</span></Table.Cell>
        <Table.Cell><div className="grid min-w-0 gap-1 font-mono text-xs">
          <span className="truncate">{listener.bind_address}:{listener.port}</span>
          <span className="truncate text-[var(--telemetry-muted)]">→ {listenerDestination(listener)}</span>
        </div></Table.Cell>
      </Table.Row>)}
    </Table.Body>
  </Table.Content></Table.ScrollContainer></Table>;
}

function listenerDestination(listener: ProxyListener) {
  if (listener.data_plane.kind === "socket") {
    const topology = listener.data_plane.settings.topology;
    if (topology.mode === "local_responder") return "LocalResponder · 无 Server 上游";
    const { host, port } = topology.settings.upstream;
    return `${host || "未配置主机"}:${port} · ${topology.settings.security.mode}`;
  }
  return listener.data_plane.settings.fixed_server?.upstream_url || "请求中的目标地址";
}
