import { Button } from "@heroui/react";
import type { RuleDefinition_Serialize } from "@/generated/rust-types";
import { ruleContentLabel, ruleDirectionLabel } from "./rule-definition-model";

export function RuleDefinitionList(props: {
  rules: RuleDefinition_Serialize[];
  selectedId?: string;
  loading: boolean;
  error?: string;
  pending: boolean;
  onNew: () => void;
  onRefresh: () => void;
  onSelect: (rule: RuleDefinition_Serialize) => void;
}) {
  return (
    <section className="min-w-0 space-y-4 overflow-auto border-r border-[var(--telemetry-line)] p-4 max-[1280px]:border-r-0" data-testid="rule-definition-list">
      <header className="flex items-center gap-2">
        <div>
          <h2 className="text-lg font-semibold">规则</h2>
          <p className="text-xs text-[var(--telemetry-muted)]">上行与下行规则统一显示；优先级只在同一方向内比较。</p>
        </div>
        <Button className="ml-auto" isDisabled={props.pending} variant="outline" onPress={props.onRefresh}>刷新</Button>
        <Button isDisabled={props.pending} variant="primary" onPress={props.onNew}>新建规则</Button>
      </header>
      {props.error && <p role="alert" className="text-sm text-red-600">{props.error}</p>}
      {props.loading && <p>正在读取规则…</p>}
      {!props.loading && props.rules.length === 0 && <p className="text-sm text-[var(--telemetry-muted)]">暂无规则，请选择新建规则开始配置</p>}
      <div className="space-y-2">
        {props.rules.map((rule) => (
          <Button
            aria-pressed={props.selectedId === rule.rule_id}
            className="flex h-auto w-full items-center justify-start gap-3 rounded-lg border border-[var(--telemetry-line)] p-3 text-left aria-pressed:border-[var(--telemetry-accent)]"
            key={rule.rule_id}
            onPress={() => props.onSelect(rule)}
            variant="ghost"
          >
            <span className="min-w-0 flex-1"><span className="block truncate font-medium">{rule.name}</span><span className="text-xs text-[var(--telemetry-muted)]">{ruleContentLabel(rule)} · priority {rule.priority}</span></span>
            <span className="rounded-full border border-[var(--telemetry-line)] px-2 py-0.5 text-xs">{ruleDirectionLabel(rule.stage)}</span>
            <span className="text-xs">{rule.enabled ? "启用" : "停用"}</span>
          </Button>
        ))}
      </div>
    </section>
  );
}
