import { Alert, Button, Chip, Spinner, Switch } from "@heroui/react";
import type {
  ProxyListener,
  SocketDocumentRuleDefinition,
} from "@/generated/rust-types";

export function SocketRulesList({
  rules,
  listeners,
  selectedId,
  loading,
  error,
  pending,
  sideEffectsDisabled = pending,
  onNew,
  onSelect,
  onToggle,
  onRetry,
}: {
  rules?: SocketDocumentRuleDefinition[];
  listeners: ProxyListener[];
  selectedId?: string;
  loading: boolean;
  error?: string;
  pending: boolean;
  sideEffectsDisabled?: boolean;
  onNew: () => void;
  onSelect: (rule: SocketDocumentRuleDefinition) => void;
  onToggle: (rule: SocketDocumentRuleDefinition, enabled: boolean) => void;
  onRetry: () => void;
}) {
  const names = new Map(listeners.map((listener) => [listener.id, listener.name]));
  return (
    <section className="min-w-0 space-y-4 overflow-auto p-5">
      <div className="flex items-center gap-3">
        <div>
          <h2 className="text-lg font-semibold">Socket 报文规则</h2>
          <p className="text-sm text-[var(--telemetry-muted)]">规则按优先级与创建顺序执行。</p>
        </div>
        <Button className="ml-auto" isDisabled={pending || listeners.length === 0} onPress={onNew} variant="primary">
          新建 Socket 规则
        </Button>
      </div>
      {error && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>Socket 规则读取失败</Alert.Title>
            <Alert.Description>{error}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={onRetry}>重试</Button>
        </Alert>
      )}
      {loading && <Spinner aria-label="正在读取 Socket 规则" />}
      {!loading && !error && listeners.length === 0 && (
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>当前 Workspace 没有 Scripted Socket Listener</Alert.Title>
            <Alert.Description>请先为 Socket Listener 绑定精确协议包。</Alert.Description>
          </Alert.Content>
        </Alert>
      )}
      {!loading && !error && listeners.length > 0 && (rules?.length ?? 0) === 0 && (
        <div className="rounded-lg border border-dashed p-8 text-center text-[var(--telemetry-muted)]">
          <p>暂无 Socket 规则</p>
          <p className="mt-1 text-sm">
            选择新建规则后绑定一个按协议处理的 Socket Listener
          </p>
        </div>
      )}
      <div aria-label="Socket 规则列表" className="space-y-2" role="list">
        {(rules ?? []).map((rule) => (
          <div
            aria-current={selectedId === rule.rule_id ? "true" : undefined}
            className="flex w-full items-center gap-3 rounded-lg border border-[var(--telemetry-line)] p-3 text-left"
            key={rule.rule_id}
            role="listitem"
          >
            <Switch
              aria-label={`${rule.enabled ? "停用" : "启用"} Socket 规则 ${rule.rule_id}`}
              isDisabled={sideEffectsDisabled}
              isSelected={rule.enabled}
              onChange={(enabled) => onToggle(rule, enabled)}
            >
              <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content>
            </Switch>
            <Button className="min-w-0 flex-1 justify-start text-left" isDisabled={pending} onPress={() => onSelect(rule)} variant="ghost">
              <span className="block truncate font-medium">{names.get(rule.listener_id) ?? rule.listener_id}</span>
              <span className="block truncate text-xs text-[var(--telemetry-muted)]">
                {rule.package.id}@{rule.package.version} · Schema v{rule.schema_version}
              </span>
              <span className="block truncate text-xs text-[var(--telemetry-muted)]">
                #{rule.rule_id.slice(0, 8)} · {rule.conditions.length} 个条件 · {rule.actions.length} 个动作
              </span>
            </Button>
            <Chip size="sm" variant="soft">{rule.direction === "upstream" ? "upstream" : "downstream"}</Chip>
            <span className="text-sm">P{rule.priority}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
