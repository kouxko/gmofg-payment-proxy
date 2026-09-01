import type { RefObject } from "react";
import {
  Alert,
  Button,
  Card,
  FieldError,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  Switch,
  TextArea,
  TextField,
} from "@heroui/react";
import type {
  ChannelId,
  ChannelPresentationViewModel,
  FaultConfigurationDraft,
  FaultParameterValue,
  FaultTemplateViewModel,
} from "@/generated/rust-types";

interface FaultConfigurationPanelProps {
  panelRef: RefObject<HTMLElement | null>;
  selected?: FaultTemplateViewModel;
  parameters: Record<string, FaultParameterValue>;
  channels: ChannelPresentationViewModel[];
  channel?: ChannelId;
  terminal: string;
  target: string;
  priority?: number;
  draft?: FaultConfigurationDraft;
  configurePending: boolean;
  writePending: boolean;
  fieldError: (field: string) => string | undefined;
  onSetParameter: (key: string, value: FaultParameterValue) => void;
  onChannelChange: (channel: ChannelId) => void;
  onTerminalChange: (value: string) => void;
  onTargetChange: (value: string) => void;
  onPriorityChange: (value: number) => void;
  onConfigure: () => void;
}

function ParameterField({
  field,
  value,
  error,
  onChange,
}: {
  field: FaultTemplateViewModel["parameter_schema"][number];
  value?: FaultParameterValue;
  error?: string;
  onChange: (value: FaultParameterValue) => void;
}) {
  if (field.kind === "boolean") {
    return (
      <div>
        <Switch
          aria-label={field.label}
          isSelected={value?.kind === "boolean" ? value.value : false}
          onChange={(next) => onChange({ kind: "boolean", value: next })}
        >
          <Switch.Content>
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
            <span>
              <span>{field.label}</span>
              <span className="block text-xs text-[var(--telemetry-muted)]">
                {field.description}
              </span>
            </span>
          </Switch.Content>
        </Switch>
        {error && <FieldError>{error}</FieldError>}
      </div>
    );
  }
  if (field.kind === "integer") {
    return (
      <div>
        <NumberField
          isInvalid={Boolean(error)}
          value={value?.kind === "integer" ? value.value : 0}
          minValue={field.minimum ?? undefined}
          maxValue={field.maximum ?? undefined}
          onChange={(next) => onChange({ kind: "integer", value: next })}
        >
          <Label>{field.label}</Label>
          <NumberField.Group className="w-full">
            <NumberField.DecrementButton />
            <NumberField.Input />
            <NumberField.IncrementButton />
          </NumberField.Group>
          {error && <FieldError>{error}</FieldError>}
        </NumberField>
        <p className="mt-1 text-xs text-[var(--telemetry-muted)]">
          {field.description}
        </p>
      </div>
    );
  }
  const text =
    value?.kind === "text" || value?.kind === "json" ? value.value : "";
  const kind = field.kind === "json" ? "json" : "text";
  return (
    <TextField isInvalid={Boolean(error)}>
      <Label>{field.label}</Label>
      {field.multiline ? (
        <TextArea
          aria-label={field.label}
          className={
            field.kind === "json"
              ? "mt-1 min-h-32 font-mono text-xs"
              : "mt-1 min-h-32"
          }
          value={text}
          onChange={(event) => onChange({ kind, value: event.target.value })}
        />
      ) : (
        <Input
          value={text}
          onChange={(event) => onChange({ kind, value: event.target.value })}
        />
      )}
      <p className="mt-1 text-xs text-[var(--telemetry-muted)]">
        {field.description}
      </p>
      {error && <FieldError>{error}</FieldError>}
    </TextField>
  );
}

export function FaultConfigurationPanel({
  panelRef,
  selected,
  parameters,
  channels,
  channel,
  terminal,
  target,
  priority,
  draft,
  configurePending,
  writePending,
  fieldError,
  onSetParameter,
  onChannelChange,
  onTerminalChange,
  onTargetChange,
  onPriorityChange,
  onConfigure,
}: FaultConfigurationPanelProps) {
  return (
    <aside
      ref={panelRef}
      className="scroll-mt-4 overflow-auto border-l border-[var(--telemetry-line)] p-5 max-[1280px]:border-l-0 max-[1280px]:border-t"
    >
      <h2 className="text-lg font-semibold">
        配置模板：{selected?.name ?? "未选择"}
      </h2>
      {selected && (
        <div className="mt-4 space-y-5">
          <Card>
            <Card.Header>
              <Card.Title>精确行为序列（网络语义）</Card.Title>
            </Card.Header>
            <Card.Content>
              <p className="text-sm">{selected.behavior_text}</p>
            </Card.Content>
          </Card>
          {selected.parameter_schema.map((field) => (
            <ParameterField
              key={field.key}
              field={field}
              value={parameters[field.key]}
              error={fieldError(`parameters.${field.key}`)}
              onChange={(value) => onSetParameter(field.key, value)}
            />
          ))}
          <Alert status={selected.ui_tone === "danger" ? "danger" : "warning"}>
            {selected.risk_text}
          </Alert>
          <div className="grid gap-1">
            <Label>代理通道</Label>
            <Select
              aria-label="代理通道"
              selectedKey={channel}
              onSelectionChange={(value) =>
                value != null && onChannelChange(value as ChannelId)
              }
            >
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>
                  {channels.map((item) => (
                    <ListBox.Item
                      key={item.id}
                      id={item.id}
                      textValue={item.display_name}
                    >
                      {item.display_name}
                    </ListBox.Item>
                  ))}
                </ListBox>
              </Select.Popover>
            </Select>
          </div>
          <TextField isInvalid={Boolean(fieldError("terminal"))}>
            <Label>终端过滤（可选）</Label>
            <Input
              aria-label="终端过滤（可选）"
              value={terminal}
              placeholder="按终端 ID 或 IP"
              onChange={(event) => onTerminalChange(event.target.value)}
            />
            {fieldError("terminal") && (
              <FieldError>{fieldError("terminal")}</FieldError>
            )}
          </TextField>
          <TextField isInvalid={Boolean(fieldError("target"))}>
            <Label>路径与请求类型</Label>
            <Input
              aria-label="路径与请求类型"
              value={target}
              placeholder="/v1/resources/example"
              onChange={(event) => onTargetChange(event.target.value)}
            />
            {fieldError("target") && (
              <FieldError>{fieldError("target")}</FieldError>
            )}
          </TextField>
          <NumberField
            isInvalid={Boolean(fieldError("priority"))}
            value={priority}
            onChange={onPriorityChange}
          >
            <Label>规则优先级</Label>
            <NumberField.Group className="w-full">
              <NumberField.DecrementButton />
              <NumberField.Input />
              <NumberField.IncrementButton />
            </NumberField.Group>
            {fieldError("priority") && (
              <FieldError>{fieldError("priority")}</FieldError>
            )}
          </NumberField>
          <Button
            variant="primary"
            isDisabled={writePending || !draft}
            onPress={onConfigure}
          >
            {configurePending ? "正在创建…" : "创建故障规则"}
          </Button>
        </div>
      )}
    </aside>
  );
}
