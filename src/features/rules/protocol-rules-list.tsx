import { Alert, Button, Chip, Spinner, Switch } from "@heroui/react";
import type {
  ProxyListener,
  ProtocolDocumentRuleDefinition,
} from "@/generated/rust-types";
import { protocolRuleStageLabel } from "./protocol-rule-model";
import type { ProtocolRuleKind } from "./protocol-rule-model";

export function ProtocolRulesList({
  kind,
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
  kind: ProtocolRuleKind;
  rules?: ProtocolDocumentRuleDefinition[];
  listeners: ProxyListener[];
  selectedId?: string;
  loading: boolean;
  error?: string;
  pending: boolean;
  sideEffectsDisabled?: boolean;
  onNew: () => void;
  onSelect: (rule: ProtocolDocumentRuleDefinition) => void;
  onToggle: (rule: ProtocolDocumentRuleDefinition, enabled: boolean) => void;
  onRetry: () => void;
}) {
  const listenerById = new Map(listeners.map((listener) => [listener.id, listener]));
  const title = kind === "http" ? "HTTP Body 报文规则" : "Socket 报文规则";
  return (
    <section className="min-w-0 space-y-4 overflow-auto p-5">
      <div className="flex items-center gap-3">
        <div>
          <h2 className="text-lg font-semibold">{title}</h2>
          <p className="text-sm text-[var(--telemetry-muted)]">规则按优先级数值从小到大逐条匹配；同优先级按创建顺序执行。</p>
        </div>
        <Button className="ml-auto" isDisabled={pending || listeners.length === 0} onPress={onNew} variant="primary">
          新建报文规则
        </Button>
      </div>
      {error && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>报文规则读取失败</Alert.Title>
            <Alert.Description>{error}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={onRetry}>重试</Button>
        </Alert>
      )}
      {loading && <Spinner aria-label="正在读取报文规则" />}
      {!loading && !error && listeners.length === 0 && (
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>当前工作区没有可配置报文规则的协议入口</Alert.Title>
            <Alert.Description>请先在入口配置中选择一个协议处理方案。</Alert.Description>
          </Alert.Content>
        </Alert>
      )}
      {!loading && !error && listeners.length > 0 && (rules?.length ?? 0) === 0 && (
        <div className="rounded-lg border border-dashed p-8 text-center text-[var(--telemetry-muted)]">
          <p>暂无报文规则</p>
          <p className="mt-1 text-sm">
            每个链路阶段单独配置，规则只在所选阶段执行。
          </p>
        </div>
      )}
      <div aria-label="报文规则列表" className="space-y-2" role="list">
        {(rules ?? []).map((rule) => (
          <div
            aria-current={selectedId === rule.rule_id ? "true" : undefined}
            className="flex w-full items-center gap-3 rounded-lg border border-[var(--telemetry-line)] p-3 text-left"
            key={rule.rule_id}
            role="listitem"
          >
            <Switch
              aria-label={`${rule.enabled ? "停用" : "启用"}报文规则 ${rule.rule_id}`}
              isDisabled={sideEffectsDisabled}
              isSelected={rule.enabled}
              onChange={(enabled) => onToggle(rule, enabled)}
            >
              <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content>
            </Switch>
            <Button className="min-w-0 flex-1 justify-start text-left" isDisabled={pending} onPress={() => onSelect(rule)} variant="ghost">
              <span className="block truncate font-medium">{rule.name}</span>
              <span className="block truncate text-xs text-[var(--telemetry-muted)]">
                {listenerById.get(rule.listener_id)?.name ?? rule.listener_id} · {rule.package.id}@{rule.package.version}
              </span>
              <span className="block truncate text-xs text-[var(--telemetry-muted)]">
                #{rule.rule_id.slice(0, 8)} · {rule.conditions.length} 个条件 · {rule.actions.length} 个动作
              </span>
            </Button>
            <Chip size="sm" variant="soft">
              {protocolRuleStageLabel(rule.stage)}
            </Chip>
            <span className="text-sm">P{rule.priority}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
