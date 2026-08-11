import { Button, Input, Label, ListBox, NumberField, Select, Tabs } from "@heroui/react";
import { Plus } from "@gravity-ui/icons";
import type { ConnectionFaultAction } from "@/generated/rust-types";
import {
  faultActionLabel,
  faultActionValue,
  updateAtIndex,
  updateFaultAction,
} from "./workspace-components-editor-model";
import {
  ComponentCard,
  type WorkspaceComponentsSectionProps,
} from "./workspace-components-editor-section";

const faultActionOptions = [
  { id: "delay", label: "连接延迟" },
  { id: "reject", label: "拒绝连接" },
  { id: "rate_limit", label: "连接限速" },
  { id: "close_after_bytes", label: "指定字节后关闭" },
  { id: "half_close_after_bytes", label: "指定字节后 half-close" },
  { id: "idle_timeout", label: "空闲超时" },
] as const;

export function FaultPresetsSection({
  workspace,
  onChange,
  onAdd,
  onIntent,
  disabled,
}: WorkspaceComponentsSectionProps) {
  return (
    <Tabs.Panel id="faults" className="space-y-3 pt-4">
      <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("fault_preset")}>
        <Plus className="size-4" />
        新增连接故障预设
      </Button>
      {workspace.fault_presets.map((preset, index) => {
        const action = preset.connection_actions[0];
        return (
          <ComponentCard
            key={preset.id}
            title="故障预设"
            index={index}
            id={preset.id}
            disabled={disabled}
            onDelete={() => onIntent("fault_preset", preset.id, "delete", "")}
          >
            <div className="grid gap-1">
              <Label>名称</Label>
              <Input
                disabled={disabled}
                value={preset.name}
                onChange={(event) =>
                  onChange({
                    ...workspace,
                    fault_presets: updateAtIndex(workspace.fault_presets, index, (item) => ({
                      ...item,
                      name: event.target.value,
                    })),
                  })
                }
              />
            </div>
            <div className="grid gap-1">
              <Label>说明</Label>
              <Input
                disabled={disabled}
                value={preset.description}
                onChange={(event) =>
                  onChange({
                    ...workspace,
                    fault_presets: updateAtIndex(workspace.fault_presets, index, (item) => ({
                      ...item,
                      description: event.target.value,
                    })),
                  })
                }
              />
            </div>
            <Select
              isDisabled={disabled}
              aria-label={`故障预设 ${index + 1} 动作`}
              selectedKey={action?.kind}
              onSelectionChange={(key) =>
                onIntent("fault_preset", preset.id, "variant", String(key))
              }
            >
              <Label>连接动作</Label>
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>
                  {faultActionOptions.map((option) => (
                    <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
                      {option.label}
                    </ListBox.Item>
                  ))}
                </ListBox>
              </Select.Popover>
            </Select>
            {action ? (
              <FaultValue
                action={action}
                disabled={disabled}
                onChange={(value) =>
                  onChange({
                    ...workspace,
                    fault_presets: updateAtIndex(workspace.fault_presets, index, (item) => ({
                      ...item,
                      connection_actions: [value],
                    })),
                  })
                }
              />
            ) : (
              <p className="text-sm text-danger">缺少连接动作，请重新选择。</p>
            )}
          </ComponentCard>
        );
      })}
    </Tabs.Panel>
  );
}

function FaultValue({
  action,
  disabled,
  onChange,
}: {
  action: ConnectionFaultAction;
  disabled: boolean;
  onChange: (value: ConnectionFaultAction) => void;
}) {
  if (action.kind === "reject") {
    return <p className="self-end text-sm text-[var(--telemetry-muted)]">该动作没有参数。</p>;
  }

  return (
    <NumberField
      isDisabled={disabled}
      minValue={1}
      value={faultActionValue(action)}
      onChange={(next) => onChange(updateFaultAction(action, next))}
    >
      <Label>{faultActionLabel(action)}</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
  );
}
