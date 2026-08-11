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

export function bytesToMib(bytes: number) {
  return Math.round(bytes / 1024 / 1024);
}

type SettingsEditorTabsProps = {
  draft: SettingsDraft;
  payloadPolicyText: string;
  fieldError: (field: string) => string | undefined;
  onDraftChange: (draft: SettingsDraft) => void;
};

export function SettingsEditorTabs({
  draft,
  payloadPolicyText,
  fieldError,
  onDraftChange,
}: SettingsEditorTabsProps) {
  return (
    <div className="min-w-0 overflow-auto max-[1280px]:overflow-visible">
      <h1 className="mb-4 text-2xl font-semibold">系统设置</h1>
      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Content className="p-0">
          <Tabs defaultSelectedKey="capacity">
            <Tabs.ListContainer>
              <Tabs.List aria-label="系统设置分类" className="px-3 pt-2">
                <Tabs.Tab id="capacity">
                  超时与容量
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="data">
                  数据与导出
                  <Tabs.Indicator />
                </Tabs.Tab>
                <Tabs.Tab id="app">
                  应用
                  <Tabs.Indicator />
                </Tabs.Tab>
              </Tabs.List>
            </Tabs.ListContainer>
            <Tabs.Panel id="capacity" className="p-4">
              <CapacitySettings
                draft={draft}
                fieldError={fieldError}
                onDraftChange={onDraftChange}
              />
            </Tabs.Panel>
            <Tabs.Panel id="data" className="space-y-4 p-4">
              <Alert status="accent">{payloadPolicyText}</Alert>
              <p className="text-sm">
                Payload 仅内存保存；规则与设置持久化；敏感导出需要确认；诊断日志不记录
                Payload、密码、私钥或 PKCS12 原始数据。
              </p>
              <Alert status="warning">
                如旧版本数据不兼容，可在页面底部清除全部配置与测试数据。此操作会停止入口、
                删除工作区、规则、设备方案、会话、抓包及导入证书，并自动重启应用。
              </Alert>
            </Tabs.Panel>
            <Tabs.Panel id="app" className="space-y-4 p-4">
              <Alert status="accent">
                系统设置只管理全局行为；入口配置、证书和规则分别在对应页面管理。
              </Alert>
              <ThemeSettings />
              <p className="text-sm">
                应用启动和诊断日志由 Rust/Tauri 桌面侧管理；外观主题仅保存在本机浏览器存储中。
              </p>
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
>;

function CapacitySettings({
  draft,
  fieldError,
  onDraftChange,
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
          onChange={(max_sessions) =>
            onDraftChange({ ...draft, max_sessions })
          }
        >
          <Label>最大会话数</Label>
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
        />
        <MemoryLimitField
          label="请求体大小限制 MiB"
          field="max_body_bytes"
          bytes={draft.max_body_bytes}
          draft={draft}
          fieldError={fieldError}
          onDraftChange={onDraftChange}
        />
        <Switch
          aria-label="Host 头重写为目标主机"
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
        待处理断点及其会话永不自动淘汰；容量判定使用 Rust 可重复计算的逻辑字节数。
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
}: MemoryLimitFieldProps) {
  return (
    <NumberField
      isInvalid={fieldError(field) != null}
      value={bytesToMib(bytes)}
      minValue={1}
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
