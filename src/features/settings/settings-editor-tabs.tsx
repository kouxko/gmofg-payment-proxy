import {
  Alert,
  Card,
  FieldError,
  Form,
  Label,
  NumberField,
  Switch,
  Tabs,
} from "@heroui/react";
import type { SettingsDraft } from "@/generated/rust-types";
import { ThemeSettings } from "./settings-content";
import { McpSettings } from "./mcp-settings";

export function bytesToMib(bytes: number) {
  return Math.round(bytes / 1024 / 1024);
}

type SettingsEditorTabsProps = {
  draft: SettingsDraft;
  fieldError: (field: string) => string | undefined;
  onDraftChange: (draft: SettingsDraft) => void;
  isDisabled: boolean;
  selectedSection: "capacity" | "app" | "mcp";
  onSectionChange: (section: "capacity" | "app" | "mcp") => void;
};

export function SettingsEditorTabs({
  draft,
  fieldError,
  onDraftChange,
  isDisabled,
  selectedSection,
  onSectionChange,
}: SettingsEditorTabsProps) {
  return (
    <div className="min-w-0">
      <h1 className="sr-only">系统设置</h1>
      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Content className="p-0">
          <Tabs
            selectedKey={selectedSection}
            onSelectionChange={(key) =>
              onSectionChange(key as "capacity" | "app" | "mcp")
            }
          >
            <Tabs.ListContainer>
              <Tabs.List aria-label="系统设置分类" className="px-3 pt-2">
                <Tabs.Tab id="capacity" isDisabled={isDisabled}>
                  超时与容量
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="app" isDisabled={isDisabled}>
                  应用
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="mcp">
                  AI 助手（MCP）
                  <Tabs.Indicator />
                </Tabs.Tab>
              </Tabs.List>
            </Tabs.ListContainer>
            <Tabs.Panel id="capacity" className="p-4">
              <CapacitySettings
                draft={draft}
                fieldError={fieldError}
                onDraftChange={onDraftChange}
                isDisabled={isDisabled}
              />
            </Tabs.Panel>
            <Tabs.Panel id="app" className="space-y-4 p-4">
              <Alert status="accent">
                系统设置只管理全局行为；入口配置、证书和规则分别在对应页面管理。
              </Alert>
              <ThemeSettings />
            </Tabs.Panel>
            <Tabs.Panel id="mcp" className="p-4">
              <McpSettings />
            </Tabs.Panel>
          </Tabs>
        </Card.Content>
      </Card>
    </div>
  );
}

type CapacitySettingsProps = Pick<
  SettingsEditorTabsProps,
  "draft" | "fieldError" | "onDraftChange"
> & { isDisabled: boolean };

function CapacitySettings({
  draft,
  fieldError,
  onDraftChange,
  isDisabled,
}: CapacitySettingsProps) {
  const timeoutFields = [
    ["连接超时（秒）", "connect_timeout_seconds"],
    ["写入超时（秒）", "write_timeout_seconds"],
    ["读取超时（秒）", "read_timeout_seconds"],
  ] as const;

  return (
    <Form className="space-y-5">
      <Alert status="accent">
        代理入口的监听地址、端口、上游和 TLS 请统一到“入口配置”中管理。
      </Alert>
      <div className="grid grid-cols-3 gap-4 max-[760px]:grid-cols-1">
        {timeoutFields.map(([label, key]) => (
          <NumberField
            key={key}
            isInvalid={fieldError(key) != null}
            value={draft[key]}
            minValue={1}
            isDisabled={isDisabled}
            onChange={(value) => onDraftChange({ ...draft, [key]: value })}
          >
            <Label>{label}</Label>
            <NumberField.Group className="w-full">
              <NumberField.DecrementButton />
              <NumberField.Input />
              <NumberField.IncrementButton />
            </NumberField.Group>
            {fieldError(key) && <FieldError>{fieldError(key)}</FieldError>}
          </NumberField>
        ))}
      </div>
      <div className="grid grid-cols-2 gap-4 max-[760px]:grid-cols-1">
        <NumberField
          isInvalid={fieldError("max_sessions") != null}
          value={draft.max_sessions}
          minValue={1}
          isDisabled={isDisabled}
          onChange={(max_sessions) =>
            onDraftChange({ ...draft, max_sessions })
          }
        >
          <Label>最多保留的 HTTP 交换</Label>
          <NumberField.Group className="w-full">
            <NumberField.DecrementButton />
            <NumberField.Input />
            <NumberField.IncrementButton />
          </NumberField.Group>
          {fieldError("max_sessions") && (
            <FieldError>{fieldError("max_sessions")}</FieldError>
          )}
        </NumberField>
        <MemoryLimitField
          label="最大内存 MiB"
          field="max_memory_bytes"
          bytes={draft.max_memory_bytes}
          draft={draft}
          fieldError={fieldError}
          onDraftChange={onDraftChange}
          isDisabled={isDisabled}
        />
        <MemoryLimitField
          label="请求体大小限制 MiB"
          field="max_body_bytes"
          bytes={draft.max_body_bytes}
          draft={draft}
          fieldError={fieldError}
          onDraftChange={onDraftChange}
          isDisabled={isDisabled}
        />
        <Switch
          aria-label="Host 头重写为目标主机"
          isDisabled={isDisabled}
          isSelected={draft.rewrite_host}
          onChange={(rewrite_host) =>
            onDraftChange({ ...draft, rewrite_host })
          }
        >
          <Switch.Content>
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
            <span>Host 头重写为目标主机</span>
          </Switch.Content>
        </Switch>
      </div>
      <Alert status="accent">
        待处理断点对应的 HTTP 交换永不自动淘汰；容量按可重复计算的逻辑字节数判定。
      </Alert>
    </Form>
  );
}

type MemoryLimitFieldProps = CapacitySettingsProps & {
  label: string;
  field: "max_memory_bytes" | "max_body_bytes";
  bytes: number;
};

function MemoryLimitField({
  label,
  field,
  bytes,
  draft,
  fieldError,
  onDraftChange,
  isDisabled,
}: MemoryLimitFieldProps) {
  return (
    <NumberField
      isInvalid={fieldError(field) != null}
      value={bytesToMib(bytes)}
      minValue={1}
      isDisabled={isDisabled}
      onChange={(value) =>
        onDraftChange({ ...draft, [field]: value * 1024 * 1024 })
      }
    >
      <Label>{label}</Label>
      <NumberField.Group className="w-full">
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
      {fieldError(field) && <FieldError>{fieldError(field)}</FieldError>}
    </NumberField>
  );
}
