import { Button, Label, ListBox, NumberField, Select } from "@heroui/react";
import type {
  BreakpointActionOptionViewModel,
  BreakpointDecision,
} from "@/generated/rust-types";

export interface BreakpointActionPanelProps {
  actions: BreakpointActionOptionViewModel[];
  selected?: BreakpointActionOptionViewModel;
  delayMs?: number;
  httpStatus?: number;
  contentLengthDelta?: number;
  truncateAt?: number;
  resolvePending: boolean;
  canResolve: boolean;
  validationValid?: boolean;
  onSelect: (kind: BreakpointDecision["kind"]) => void;
  onDelayChange: (value: number) => void;
  onHttpStatusChange: (value: number) => void;
  onContentLengthDeltaChange: (value: number) => void;
  onTruncateAtChange: (value: number) => void;
  onResolve: (kind: BreakpointDecision["kind"]) => void;
  compact?: boolean;
}

function ActionParameters(props: BreakpointActionPanelProps) {
  const kind = props.selected?.kind;
  const field = (
    label: string,
    value: number,
    onChange: (value: number) => void,
    min?: number,
    max?: number,
  ) => (
    <NumberField
      value={value}
      minValue={min}
      maxValue={max}
      onChange={onChange}
    >
      <Label>{label}</Label>
      <NumberField.Group className="w-full">
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
  );
  if (kind === "delay" && props.selected?.default_delay_ms != null)
    return field(
      "延迟毫秒",
      props.delayMs ?? props.selected.default_delay_ms,
      props.onDelayChange,
      0,
    );
  if (
    kind === "custom_http_status" &&
    props.selected?.default_http_status != null
  )
    return field(
      "HTTP 状态码",
      props.httpStatus ?? props.selected.default_http_status,
      props.onHttpStatusChange,
      100,
      599,
    );
  if (
    kind === "wrong_content_length" &&
    props.selected?.default_content_length_delta != null
  )
    return field(
      "Content-Length 差值",
      props.contentLengthDelta ?? props.selected.default_content_length_delta,
      props.onContentLengthDeltaChange,
    );
  if (kind === "truncate" && props.selected?.default_truncate_at != null)
    return field(
      "截断字节位置",
      props.truncateAt ?? props.selected.default_truncate_at,
      props.onTruncateAtChange,
      0,
    );
  return null;
}

export function BreakpointActionPanel(props: BreakpointActionPanelProps) {
  const disabled =
    props.resolvePending ||
    !props.canResolve ||
    props.validationValid === false;
  return (
    <div
      className={
        props.compact
          ? "space-y-4"
          : "overflow-auto border-l border-[var(--telemetry-line)] p-4 max-[1280px]:hidden"
      }
    >
      {!props.compact && (
        <h2 className="mb-5 text-lg font-semibold">处理方式</h2>
      )}
      <Select
        aria-label="断点处理方式"
        selectedKey={props.selected?.kind}
        onSelectionChange={(key) =>
          props.onSelect(key as BreakpointDecision["kind"])
        }
      >
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            {props.actions.map((action) => (
              <ListBox.Item
                key={action.kind}
                id={action.kind}
                isDisabled={!action.enabled}
              >
                {action.label}
              </ListBox.Item>
            ))}
          </ListBox>
        </Select.Popover>
      </Select>
      <div className="mt-4 space-y-3">
        <ActionParameters {...props} />
      </div>
      {!props.compact && (
        <div className="mt-8 space-y-3">
          <Button
            fullWidth
            variant="primary"
            isDisabled={disabled}
            onPress={() =>
              props.selected && props.onResolve(props.selected.kind)
            }
          >
            {props.resolvePending ? "正在处理…" : "执行所选处理"}
          </Button>
          {props.actions
            .filter((action) => action.kind === "forward_original")
            .map((action) => (
              <Button
                key={action.kind}
                fullWidth
                variant="outline"
                isDisabled={props.resolvePending || !action.enabled}
                onPress={() => props.onResolve(action.kind)}
              >
                {action.label}
              </Button>
            ))}
          {props.actions
            .filter((action) => action.kind === "disconnect_before_upstream")
            .map((action) => (
              <Button
                key={action.kind}
                fullWidth
                variant="danger-soft"
                isDisabled={props.resolvePending || !action.enabled}
                onPress={() => props.onResolve(action.kind)}
              >
                {action.label}
              </Button>
            ))}
        </div>
      )}
    </div>
  );
}
