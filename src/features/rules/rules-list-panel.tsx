import { Alert, Button, Chip, Switch, Table } from "@heroui/react";
import { FileArrowRightOut, FileArrowUp, Plus } from "@gravity-ui/icons";
import type { RuleSummaryViewModel } from "@/generated/rust-types";
import { formatTimestamp } from "@/lib/format";

type RulesListPanelProps = {
  rules?: RuleSummaryViewModel[];
  error?: string;
  isLoading: boolean;
  selectedId?: string;
  writePending: boolean;
  editorBlocked: boolean;
  pendingAction?: string;
  onNew: () => void;
  onImport: () => void;
  onExport: () => void;
  onRefresh: () => void;
  onSelect: (ruleId: string) => void;
  onToggle: (rule: RuleSummaryViewModel, enabled: boolean) => void;
};

export function RulesListPanel({
  rules,
  error,
  isLoading,
  selectedId,
  writePending,
  editorBlocked,
  pendingAction,
  onNew,
  onImport,
  onExport,
  onRefresh,
  onSelect,
  onToggle,
}: RulesListPanelProps) {
  return (
    <div className="min-w-0 space-y-5 overflow-auto p-5">
      <div className="flex items-center">
        <h2 className="text-lg font-semibold">HTTP 拦截规则</h2>
        <div className="ml-auto flex gap-3">
          <Button
            variant="primary"
            isDisabled={writePending || editorBlocked}
            onPress={onNew}
          >
            <Plus className="size-4" />
            {pendingAction === "new" ? "正在新建…" : "新建规则"}
          </Button>
          <Button
            variant="outline"
            isDisabled={writePending}
            onPress={onImport}
          >
            <FileArrowUp className="size-4" />
            {pendingAction === "import" ? "正在导入…" : "导入规则"}
          </Button>
          <Button
            variant="outline"
            isDisabled={writePending}
            onPress={onExport}
          >
            <FileArrowRightOut className="size-4" />
            {pendingAction === "export" ? "正在导出…" : "导出规则"}
          </Button>
        </div>
      </div>
      {error && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>规则列表读取失败</Alert.Title>
            <Alert.Description>{error}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={onRefresh}>
            重试
          </Button>
        </Alert>
      )}
      <Table>
        <Table.ScrollContainer>
          <Table.Content
            aria-label="拦截规则"
            className="min-w-[1080px]"
            selectionMode="single"
            selectedKeys={selectedId ? [selectedId] : []}
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              const first = Array.from(keys)[0];
              if (first != null) onSelect(String(first));
            }}
          >
            <Table.Header>
              <Table.Column>启用</Table.Column>
              <Table.Column>优先级</Table.Column>
              <Table.Column isRowHeader>规则名称</Table.Column>
              <Table.Column>通道</Table.Column>
              <Table.Column>阶段</Table.Column>
              <Table.Column>匹配条件（摘要）</Table.Column>
              <Table.Column>执行动作（摘要）</Table.Column>
              <Table.Column>命中数</Table.Column>
              <Table.Column>最后命中时间</Table.Column>
            </Table.Header>
            <Table.Body
              renderEmptyState={() => (
                <div className="p-8 text-center">
                  {isLoading
                    ? "正在读取规则…"
                    : error
                      ? "规则列表暂不可用"
                      : "暂无 HTTP 拦截规则，请选择新建规则开始配置"}
                </div>
              )}
            >
              {(rules ?? []).map((rule) => (
                <Table.Row key={rule.rule_id} id={rule.rule_id}>
                  <Table.Cell>
                    <Switch
                      aria-label={`${rule.enabled ? "停用" : "启用"}规则 ${rule.name}`}
                      isSelected={rule.enabled}
                      isDisabled={writePending || editorBlocked}
                      onChange={(enabled) => onToggle(rule, enabled)}
                    >
                      <Switch.Content>
                        <Switch.Control>
                          <Switch.Thumb />
                        </Switch.Control>
                        <span className="sr-only">
                          {rule.enabled ? "停用规则" : "启用规则"}
                        </span>
                      </Switch.Content>
                    </Switch>
                  </Table.Cell>
                  <Table.Cell>{rule.priority}</Table.Cell>
                  <Table.Cell className="font-medium">{rule.name}</Table.Cell>
                  <Table.Cell>
                    <Chip size="sm" color="accent" variant="soft">
                      {rule.channel_text}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell>{rule.stage_text}</Table.Cell>
                  <Table.Cell>{rule.match_summary}</Table.Cell>
                  <Table.Cell>{rule.action_summary}</Table.Cell>
                  <Table.Cell>{rule.hit_count}</Table.Cell>
                  <Table.Cell>{formatTimestamp(rule.last_hit_at)}</Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
      </Table>
      <Alert status="accent">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>执行顺序</Alert.Title>
          <Alert.Description>
            按优先级升序、同优先级按创建顺序执行；命中终止动作后停止后续规则。
          </Alert.Description>
        </Alert.Content>
      </Alert>
    </div>
  );
}
