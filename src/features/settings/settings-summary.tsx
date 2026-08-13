import { Accordion, Alert, Button, Card, Chip } from "@heroui/react";
import type {
  FieldValidationViewModel,
  SettingsDraft,
} from "@/generated/rust-types";
import { bytesToMib } from "./settings-editor-tabs";

type SettingsSummaryProps = {
  stored: SettingsDraft;
  draftDirty: boolean;
  validation?: FieldValidationViewModel;
  writePending: boolean;
  validating: boolean;
  onValidate: () => void;
};

export function SettingsSummary({
  stored,
  draftDirty,
  validation,
  writePending,
  validating,
  onValidate,
}: SettingsSummaryProps) {
  return (
    <aside className="overflow-auto max-[1280px]:mt-4 max-[1280px]:overflow-visible">
      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Header>
          <Card.Title>配置摘要与校验</Card.Title>
        </Card.Header>
        <Card.Content className="space-y-4">
          <Accordion defaultExpandedKeys={["stored", "pending", "validation"]}>
            <Accordion.Item id="stored">
              <Accordion.Heading>
                <Accordion.Trigger>
                  已保存的全局设置
                  <Accordion.Indicator />
                </Accordion.Trigger>
              </Accordion.Heading>
              <Accordion.Panel>
                <Accordion.Body>
                  <StoredSettings stored={stored} />
                </Accordion.Body>
              </Accordion.Panel>
            </Accordion.Item>
            <Accordion.Item id="pending">
              <Accordion.Heading>
                <Accordion.Trigger>
                  保存与生效状态
                  <Accordion.Indicator />
                </Accordion.Trigger>
              </Accordion.Heading>
              <Accordion.Panel>
                <Accordion.Body className="flex flex-wrap gap-2">
                  <Chip
                    color={draftDirty ? "warning" : "success"}
                    variant="soft"
                  >
                    {draftDirty ? "存在未保存草稿" : "草稿与已保存设置一致"}
                  </Chip>
                </Accordion.Body>
              </Accordion.Panel>
            </Accordion.Item>
            <Accordion.Item id="validation">
              <Accordion.Heading>
                <Accordion.Trigger>
                  校验结果
                  <Accordion.Indicator />
                </Accordion.Trigger>
              </Accordion.Heading>
              <Accordion.Panel>
                <Accordion.Body>
                  {!validation ? (
                    <Button
                      variant="outline"
                      isDisabled={writePending}
                      onPress={onValidate}
                    >
                      {validating ? "正在校验…" : "校验设置"}
                    </Button>
                  ) : (
                    <Alert status={validation.valid ? "success" : "danger"}>
                      {validation.valid
                        ? validation.warnings.join("；") || "全部检查通过。"
                        : Object.values(validation.field_errors)
                            .flat()
                            .join("；")}
                    </Alert>
                  )}
                </Accordion.Body>
              </Accordion.Panel>
            </Accordion.Item>
          </Accordion>
        </Card.Content>
      </Card>
    </aside>
  );
}

function StoredSettings({ stored }: { stored: SettingsDraft }) {
  return (
    <>
      <dl className="grid grid-cols-[120px_1fr] gap-y-2 text-sm">
        <dt>连接超时</dt>
        <dd>{stored.connect_timeout_seconds} 秒</dd>
        <dt>写入超时</dt>
        <dd>{stored.write_timeout_seconds} 秒</dd>
        <dt>读取超时</dt>
        <dd>{stored.read_timeout_seconds} 秒</dd>
        <dt>最大会话数</dt>
        <dd>{stored.max_sessions}</dd>
        <dt>最大内存</dt>
        <dd>{bytesToMib(stored.max_memory_bytes)} MiB</dd>
      </dl>
      <p className="mt-3 text-xs text-[var(--telemetry-muted)]">
        监听端口、请求去向和入口启停不属于系统设置。
      </p>
    </>
  );
}
