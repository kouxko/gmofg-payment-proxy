import type { ReactNode } from "react";
import { Input, Label, ListBox, NumberField, Select, Switch, TextField } from "@heroui/react";
import type { RuleStage } from "@/generated/rust-types";
import { ruleStageLabel } from "./rule-definition-model";

export type RuleMetadataStageOption = { stage: RuleStage; reason?: string | null };

export function RuleMetadataFields(props: {
  enabled: boolean;
  listenerControl: ReactNode;
  name: string;
  pending: boolean;
  priority?: number;
  stage?: RuleStage;
  stageOptions: RuleMetadataStageOption[];
  onEnabledChange: (enabled: boolean) => void;
  onNameChange: (name: string) => void;
  onPriorityChange: (priority: number) => void;
  onStageChange: (stage: RuleStage) => void;
}) {
  return <section className="space-y-4" data-testid="rule-metadata-fields">
    <TextField isDisabled={props.pending}><Label>规则名称</Label><Input aria-label="规则名称" maxLength={128} value={props.name} onChange={(event) => props.onNameChange(event.target.value)} /></TextField>
    <div className="grid gap-3 sm:grid-cols-2">
      {props.listenerControl}
      <Select aria-label="处理阶段" isDisabled={props.pending || props.stageOptions.length === 0} selectedKey={props.stage ?? null} onSelectionChange={(key) => props.onStageChange(String(key) as RuleStage)}>
        <Label>处理阶段</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{props.stageOptions.map(({ stage, reason }) => <ListBox.Item id={stage} isDisabled={reason != null} key={stage} textValue={`${ruleStageLabel(stage)}${reason ? ` ${reason}` : ""}`}><span className="block">{ruleStageLabel(stage)}</span>{reason && <span className="block text-xs text-red-600">{reason}</span>}</ListBox.Item>)}</ListBox></Select.Popover>
      </Select>
    </div>
    <div className="grid items-end gap-3 sm:grid-cols-2" data-testid="rule-metadata-toggle-priority-row">
      <Switch aria-label="启用规则" isDisabled={props.pending} isSelected={props.enabled} onChange={props.onEnabledChange}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control>启用规则</Switch.Content></Switch>
      <NumberField aria-label="阶段内优先级" isDisabled={props.pending} minValue={0} value={props.priority ?? Number.NaN} onChange={props.onPriorityChange}><Label>阶段内优先级</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
    </div>
    {props.stage && <p className="text-xs text-[var(--telemetry-muted)]">{ruleStageLabel(props.stage)} · priority 只与此阶段及同一执行作用域中的规则比较。</p>}
  </section>;
}
