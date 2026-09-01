import { Button } from "@heroui/react";
import type { Condition, UnifiedAction } from "@/generated/rust-types";

export function FlatConditionList({ conditions, onChange }: {
  conditions: Condition[];
  onChange: (conditions: Condition[]) => void;
}) {
  return (
    <section className="space-y-2" aria-labelledby="flat-conditions-heading">
      <div>
        <h5 className="text-sm font-medium" id="flat-conditions-heading">匹配条件</h5>
        <p className="text-xs text-[var(--telemetry-muted)]">所有条件固定为 AND；需要 OR 时请新建多条规则。</p>
      </div>
      <ol className="space-y-2">
        {conditions.map((condition, index) => (
          <li className="flex items-center gap-2 rounded-md border border-[var(--telemetry-line)] p-2 text-xs" key={index}>
            <span>{index + 1}. {conditionLabel(condition)}</span>
            <Button aria-label={`删除条件 ${index + 1}`} className="ml-auto" size="sm" variant="ghost" onPress={() => onChange(conditions.filter((_, itemIndex) => itemIndex !== index))}>删除</Button>
          </li>
        ))}
      </ol>
    </section>
  );
}

export function OrderedActionList({ actions, label, onChange }: {
  actions: UnifiedAction[];
  label: (action: UnifiedAction) => string;
  onChange: (actions: UnifiedAction[]) => void;
}) {
  return (
    <section className="space-y-2" aria-labelledby="ordered-actions-heading">
      <h5 className="text-sm font-medium" id="ordered-actions-heading">有序动作列表</h5>
      <ol className="space-y-2">
        {actions.map((action, index) => (
          <li className="flex items-center gap-2 rounded-md border border-[var(--telemetry-line)] p-2 text-xs" key={index}>
            <span>{index + 1}. {label(action)}</span>
            <div className="ml-auto flex gap-1">
              <Button aria-label={`上移动作 ${index + 1}`} isDisabled={index === 0} size="sm" variant="ghost" onPress={() => onChange(move(actions, index, index - 1))}>上移</Button>
              <Button aria-label={`下移动作 ${index + 1}`} isDisabled={index === actions.length - 1} size="sm" variant="ghost" onPress={() => onChange(move(actions, index, index + 1))}>下移</Button>
              <Button aria-label={`删除动作 ${index + 1}`} isDisabled={actions.length === 1} size="sm" variant="ghost" onPress={() => onChange(actions.filter((_, itemIndex) => itemIndex !== index))}>删除</Button>
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}

function conditionLabel(condition: Condition): string {
  if (condition.source === "document" || condition.source === "document_pattern") return `${condition.path || "/"} · ${condition.predicate.type}`;
  if (condition.source === "nth_hit") return `第 ${condition.count} 次命中`;
  if (condition.field === "Method") return `Method · ${operatorLabel(condition.operator)}`;
  if (condition.field === "RequestTarget") return `URL · ${operatorLabel(condition.operator)}`;
  if (typeof condition.field === "object" && "Header" in condition.field) return `Header ${condition.field.Header} · ${operatorLabel(condition.operator)}`;
  return `${String(condition.field)} · ${operatorLabel(condition.operator)}`;
}

function operatorLabel(operator: Extract<Condition, { source: "http" }>["operator"]): string {
  if ("Equals" in operator) return `equals ${operator.Equals}`;
  if ("Contains" in operator) return `contains ${operator.Contains}`;
  if ("StartsWith" in operator) return `starts with ${operator.StartsWith}`;
  if ("EndsWith" in operator) return `ends with ${operator.EndsWith}`;
  return `wildcard ${operator.Wildcard}`;
}

function move<T>(items: T[], from: number, to: number): T[] {
  const result = [...items];
  const [item] = result.splice(from, 1);
  result.splice(to, 0, item);
  return result;
}
