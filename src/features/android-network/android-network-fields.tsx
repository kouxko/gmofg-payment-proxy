import type { ReactElement } from "react";
import { Label, NumberField } from "@heroui/react";

interface NumericFieldProps {
  label: string;
  value: number;
  onChange: (value: number) => void;
  ariaLabel?: string;
  minValue?: number;
  maxValue?: number;
}

export function NumericField({
  label,
  value,
  onChange,
  ariaLabel,
  minValue,
  maxValue,
}: NumericFieldProps): ReactElement {
  return (
    <NumberField
      aria-label={ariaLabel}
      minValue={minValue}
      maxValue={maxValue}
      value={value}
      onChange={onChange}
    >
      <Label>{label}</Label>
      <NumberField.Group>
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
  );
}
