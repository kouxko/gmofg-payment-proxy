import { Label, ListBox, NumberField, Select } from "@heroui/react";
import type { RuleTrafficDirection } from "@/generated/rust-types";

export function NumericInput({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <NumberField value={value} onChange={onChange}>
      <Label>{label}</Label>
      <NumberField.Group className="w-full">
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
  );
}

export function TrafficDirectionSelect({
  value,
  onChange,
}: {
  value: RuleTrafficDirection;
  onChange: (value: RuleTrafficDirection) => void;
}) {
  return (
    <div className="grid gap-1">
      <Label>流量方向</Label>
      <Select
        aria-label="流量方向"
        selectedKey={value}
        onSelectionChange={(direction) =>
          onChange(direction as RuleTrafficDirection)
        }
      >
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            <ListBox.Item id="upstream">上行：Proxy → Server</ListBox.Item>
            <ListBox.Item id="downstream">下行：Proxy → App</ListBox.Item>
          </ListBox>
        </Select.Popover>
      </Select>
    </div>
  );
}
