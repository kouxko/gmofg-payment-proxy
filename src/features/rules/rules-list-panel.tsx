import { Alert, Button, Chip, Switch, Table } from "@heroui/react";
import { FileArrowRightOut, FileArrowUp, Plus } from "@gravity-ui/icons";
import type {
  ProtocolDocumentRuleDefinition,
  RuleSummaryViewModel,
} from "@/generated/rust-types";
import { formatTimestamp } from "@/lib/format";
import { protocolRuleStageLabel } from "./protocol-rule-model";

type RulesListPanelProps = {
  rules?: RuleSummaryViewModel[];
  bodyRules?: ProtocolDocumentRuleDefinition[];
  bodyListenerNames?: Map<string, string>;
  error?: string;
  isLoading: boolean;
  selectedId?: string;
  selectedKind?: "standard" | "body";
  writePending: boolean;
  editorBlocked: boolean;
  pendingAction?: string;
  onNew: () => void;
  onImport: () => void;
  onExport: () => void;
  onRefresh: () => void;
  onSelect: (ruleId: string) => void;
  onSelectBody?: (ruleId: string) => void;
  onToggle: (rule: RuleSummaryViewModel, enabled: boolean) => void;
  onToggleBody?: (
    rule: ProtocolDocumentRuleDefinition,
    enabled: boolean,
  ) => void;
};

export function RulesListPanel({
  rules,
  bodyRules = [],
  bodyListenerNames = new Map(),
  error,
  isLoading,
  selectedId,
  selectedKind = "standard",
  writePending,
  editorBlocked,
  pendingAction,
  onNew,
  onImport,
  onExport,
  onRefresh,
  onSelect,
  onSelectBody,
  onToggle,
  onToggleBody,
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
            selectedKeys={selectedId ? [`${selectedKind}:${selectedId}`] : []}
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              const first = Array.from(keys)[0];
              if (first == null) return;
              const [kind, ...idParts] = String(first).split(":");
              const ruleId = idParts.join(":");
              if (kind === "body") onSelectBody?.(ruleId);
              else onSelect(ruleId);
            }}
          >
            <Table.Header>
              <Table.Column>启用</Table.Column>
              <Table.Column>优先级</Table.Column>
              <Table.Column isRowHeader>规则名称</Table.Column>
              <Table.Column>作用范围</Table.Column>
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
                <Table.Row
                  key={`standard:${rule.rule_id}`}
                  id={`standard:${rule.rule_id}`}
                >
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
              {bodyRules.map((rule) => (
                <Table.Row
                  key={`body:${rule.rule_id}`}
                  id={`body:${rule.rule_id}`}
                >
                  <Table.Cell>
                    <Switch
                      aria-label={`${rule.enabled ? "停用" : "启用"} Body 报文规则 ${rule.name}`}
                      isSelected={rule.enabled}
                      isDisabled={writePending || editorBlocked}
                      onChange={(enabled) => onToggleBody?.(rule, enabled)}
                    >
                      <Switch.Content>
                        <Switch.Control>
                          <Switch.Thumb />
                        </Switch.Control>
                      </Switch.Content>
                    </Switch>
                  </Table.Cell>
                  <Table.Cell>{rule.priority}</Table.Cell>
                  <Table.Cell className="font-medium">{rule.name}</Table.Cell>
                  <Table.Cell>
                    <div className="flex items-center gap-2">
                      <Chip size="sm" color="accent" variant="soft">Body 报文</Chip>
                      <span>{bodyListenerNames.get(rule.listener_id) ?? "—"}</span>
                    </div>
                  </Table.Cell>
                  <Table.Cell>{protocolRuleStageLabel(rule.stage)}</Table.Cell>
                  <Table.Cell>{rule.conditions.length} 个条件</Table.Cell>
                  <Table.Cell>{rule.actions.length} 个动作</Table.Cell>
                  <Table.Cell>—</Table.Cell>
                  <Table.Cell>—</Table.Cell>
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
            先执行 HTTP 基础规则，再执行 Body 报文规则；每类内部按优先级升序、
            同优先级按创建顺序执行，命中终止动作后停止该类后续规则。
          </Alert.Description>
        </Alert.Content>
      </Alert>
    </div>
  );
}
