import { Alert, Button, Chip, Table } from "@heroui/react";
import type { FaultTemplateViewModel } from "@/generated/rust-types";
import { toneColor } from "@/lib/format";

interface QueryState<T> {
  data?: T;
  error?: string;
  isLoading: boolean;
  refresh: () => Promise<void>;
}

interface FaultsListPanelProps {
  templates: QueryState<FaultTemplateViewModel[]>;
  effectiveSelectedId?: string;
  hasChannels: boolean;
  onSelectTemplate: (templateId: string) => void;
}

function EmptyState({
  loading,
  error,
  empty,
}: {
  loading: string;
  error: string;
  empty: string;
}) {
  return <div className="p-8 text-center">{loading || error || empty}</div>;
}

export function FaultsListPanel({
  templates,
  effectiveSelectedId,
  hasChannels,
  onSelectTemplate,
}: FaultsListPanelProps) {
  return (
    <div className="min-w-0 space-y-4 overflow-auto p-5">
      <Alert status="accent">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>故障模板最终创建普通拦截规则</Alert.Title>
          <Alert.Description>
            复杂条件可在规则管理继续编辑，不建立第二套执行引擎。
          </Alert.Description>
        </Alert.Content>
      </Alert>
      {!hasChannels && (
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>当前 Workspace 没有代理入口</Alert.Title>
            <Alert.Description>
              请先在“代理入口配置”中新增 HTTP 入口，故障预设才能绑定到实际流量通道。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      )}

      <section className="min-w-0">
        <h2 className="mb-3 text-lg font-semibold">
          故障模板（快速启用安全的故障场景）
        </h2>
        {templates.error && (
          <Alert status="danger" className="mb-3">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>故障模板读取失败</Alert.Title>
              <Alert.Description>{templates.error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void templates.refresh()}
            >
              重试
            </Button>
          </Alert>
        )}
        <Table>
          <Table.ScrollContainer>
            <Table.Content
              aria-label="故障模板"
              className="min-w-[820px]"
              selectionMode="single"
              selectedKeys={effectiveSelectedId ? [effectiveSelectedId] : []}
              onSelectionChange={(keys) => {
                if (keys === "all") return;
                const first = Array.from(keys)[0];
                if (first != null) onSelectTemplate(String(first));
              }}
            >
              <Table.Header>
                <Table.Column>阶段</Table.Column>
                <Table.Column isRowHeader>行为（精确语义）</Table.Column>
                <Table.Column>影响端</Table.Column>
                <Table.Column>默认参数</Table.Column>
                <Table.Column>风险</Table.Column>
              </Table.Header>
              <Table.Body
                renderEmptyState={() => (
                  <EmptyState
                    loading={templates.isLoading ? "正在读取故障模板…" : ""}
                    error={
                      !templates.isLoading && templates.error
                        ? "故障模板暂不可用"
                        : ""
                    }
                    empty={
                      !templates.isLoading && !templates.error
                        ? "暂无故障模板"
                        : ""
                    }
                  />
                )}
              >
                {(templates.data ?? []).map((template) => (
                  <Table.Row
                    key={template.template_id}
                    id={template.template_id}
                  >
                    <Table.Cell>{template.stage_text}</Table.Cell>
                    <Table.Cell>
                      <div className="font-medium">{template.name}</div>
                      <div className="text-xs text-[var(--telemetry-muted)]">
                        {template.behavior_text}
                      </div>
                    </Table.Cell>
                    <Table.Cell>{template.affected_party_text}</Table.Cell>
                    <Table.Cell className="max-w-56 text-xs">
                      {Object.entries(template.default_parameters)
                        .map(([key, value]) => `${key}: ${value.value}`)
                        .join("；") || "—"}
                    </Table.Cell>
                    <Table.Cell>
                      <Chip
                        size="sm"
                        color={toneColor(template.ui_tone)}
                        variant="soft"
                      >
                        {template.risk_text}
                      </Chip>
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
      </section>
    </div>
  );
}
