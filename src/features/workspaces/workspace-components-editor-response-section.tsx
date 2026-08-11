import { Button, Input, Label, ListBox, NumberField, Select, Switch, Tabs } from "@heroui/react";
import { Plus } from "@gravity-ui/icons";
import type { ResponseAssertionKind } from "@/generated/rust-types";
import { updateAtIndex } from "./workspace-components-editor-model";
import {
  ComponentCard,
  type WorkspaceComponentsSectionProps,
} from "./workspace-components-editor-section";

const assertionKindOptions = [
  { id: "http_status_equals", label: "HTTP 状态码等于" },
  { id: "header_equals", label: "Header 等于" },
  { id: "json_path_equals", label: "JSONPath 等于" },
  { id: "body_text_contains", label: "Body 文本包含" },
  { id: "body_length_equals", label: "Body 长度等于" },
  { id: "body_sha256_equals", label: "Body SHA-256 等于" },
] as const;

export function ResponseAssertionsSection({
  workspace,
  onChange,
  onAdd,
  onIntent,
  disabled,
}: WorkspaceComponentsSectionProps) {
  return (
    <Tabs.Panel id="assertions" className="space-y-3 pt-4">
      <Button variant="outline" isDisabled={disabled} onPress={() => onAdd("response_assertion")}>
        <Plus className="size-4" />
        新增响应断言
      </Button>
      {workspace.response_assertions.map((assertion, index) => (
        <ComponentCard
          key={assertion.id}
          title="响应断言"
          index={index}
          id={assertion.id}
          disabled={disabled}
          onDelete={() => onIntent("response_assertion", assertion.id, "delete", "")}
          trailing={(
            <Switch
              className="ml-auto"
              isDisabled={disabled}
              isSelected={assertion.enabled}
              onChange={(enabled) =>
                onChange({
                  ...workspace,
                  response_assertions: updateAtIndex(
                    workspace.response_assertions,
                    index,
                    (item) => ({ ...item, enabled }),
                  ),
                })
              }
            >
              <Switch.Content>
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
                <span>启用</span>
              </Switch.Content>
            </Switch>
          )}
        >
          <div className="grid gap-1">
            <Label>名称</Label>
            <Input
              disabled={disabled}
              value={assertion.name}
              onChange={(event) =>
                onChange({
                  ...workspace,
                  response_assertions: updateAtIndex(
                    workspace.response_assertions,
                    index,
                    (item) => ({ ...item, name: event.target.value }),
                  ),
                })
              }
            />
          </div>
          <div className="grid gap-1">
            <Label>代理入口 ID（逗号分隔）</Label>
            <Input
              disabled={disabled}
              key={`${assertion.id}:${assertion.listener_ids.join(",")}`}
              defaultValue={assertion.listener_ids.join(", ")}
              onBlur={(event) =>
                onIntent("response_assertion", assertion.id, "listener_ids", event.target.value)
              }
            />
          </div>
          <Select
            isDisabled={disabled}
            aria-label={`响应断言 ${index + 1} 类型`}
            selectedKey={assertion.assertion.kind}
            onSelectionChange={(key) =>
              onIntent("response_assertion", assertion.id, "variant", String(key))
            }
          >
            <Label>断言类型</Label>
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {assertionKindOptions.map((option) => (
                  <ListBox.Item key={option.id} id={option.id} textValue={option.label}>
                    {option.label}
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <AssertionInputs
            assertion={assertion.assertion}
            disabled={disabled}
            onChange={(value) =>
              onChange({
                ...workspace,
                response_assertions: updateAtIndex(
                  workspace.response_assertions,
                  index,
                  (item) => ({ ...item, assertion: value }),
                ),
              })
            }
          />
        </ComponentCard>
      ))}
    </Tabs.Panel>
  );
}

function AssertionInputs({
  assertion,
  disabled,
  onChange,
}: {
  assertion: ResponseAssertionKind;
  disabled: boolean;
  onChange: (value: ResponseAssertionKind) => void;
}) {
  switch (assertion.kind) {
    case "http_status_equals":
      return (
        <NumberField
          isDisabled={disabled}
          minValue={100}
          maxValue={599}
          value={assertion.expected}
          onChange={(expected) => onChange({ ...assertion, expected })}
        >
          <Label>期望状态码</Label>
          <NumberField.Group>
            <NumberField.DecrementButton />
            <NumberField.Input />
            <NumberField.IncrementButton />
          </NumberField.Group>
        </NumberField>
      );
    case "body_length_equals":
      return (
        <NumberField
          isDisabled={disabled}
          minValue={0}
          value={assertion.expected}
          onChange={(expected) => onChange({ ...assertion, expected })}
        >
          <Label>期望字节数</Label>
          <NumberField.Group>
            <NumberField.DecrementButton />
            <NumberField.Input />
            <NumberField.IncrementButton />
          </NumberField.Group>
        </NumberField>
      );
    case "header_equals":
      return (
        <div className="grid grid-cols-2 gap-2">
          <div className="grid gap-1">
            <Label>Header 名</Label>
            <Input
              disabled={disabled}
              value={assertion.name}
              onChange={(event) => onChange({ ...assertion, name: event.target.value })}
            />
          </div>
          <div className="grid gap-1">
            <Label>期望值</Label>
            <Input
              disabled={disabled}
              value={assertion.expected}
              onChange={(event) => onChange({ ...assertion, expected: event.target.value })}
            />
          </div>
        </div>
      );
    case "json_path_equals":
      return (
        <div className="grid grid-cols-2 gap-2">
          <div className="grid gap-1">
            <Label>JSONPath</Label>
            <Input
              disabled={disabled}
              value={assertion.path}
              onChange={(event) => onChange({ ...assertion, path: event.target.value })}
            />
          </div>
          <div className="grid gap-1">
            <Label>期望值</Label>
            <Input
              disabled={disabled}
              value={String(assertion.expected ?? "")}
              onChange={(event) => onChange({ ...assertion, expected: event.target.value })}
            />
          </div>
        </div>
      );
    case "body_text_contains":
      return (
        <div className="grid gap-1">
          <Label>Body 必须包含</Label>
          <Input
            disabled={disabled}
            value={assertion.expected}
            onChange={(event) => onChange({ ...assertion, expected: event.target.value })}
          />
        </div>
      );
    case "body_sha256_equals":
      return (
        <div className="grid gap-1">
          <Label>SHA-256（64 位十六进制）</Label>
          <Input
            disabled={disabled}
            value={assertion.expected_hex}
            onChange={(event) => onChange({ ...assertion, expected_hex: event.target.value })}
          />
        </div>
      );
  }
}
