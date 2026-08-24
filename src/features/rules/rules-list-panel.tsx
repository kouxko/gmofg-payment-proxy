import { Alert, Button, Chip, Switch, Table } from "@heroui/react";
import { Plus } from "@gravity-ui/icons";
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
  socketRules?: ProtocolDocumentRuleDefinition[];
  socketListenerNames?: Map<string, string>;
  error?: string;
  isLoading: boolean;
  selectedId?: string;
  selectedKind?: "standard" | "body" | "socket";
  writePending: boolean;
  editorBlocked: boolean;
  pendingAction?: string;
  onNew: () => void;
  onRefresh: () => void;
  onSelect: (ruleId: string) => void;
  onSelectProtocol?: (kind: "body" | "socket", ruleId: string) => void;
  onToggle: (rule: RuleSummaryViewModel, enabled: boolean) => void;
  onToggleProtocol?: (
    kind: "body" | "socket",
    rule: ProtocolDocumentRuleDefinition,
    enabled: boolean,
  ) => void;
};

export function RulesListPanel({
  rules,
  bodyRules = [],
  bodyListenerNames = new Map(),
  socketRules = [],
  socketListenerNames = new Map(),
  error,
  isLoading,
  selectedId,
  selectedKind = "standard",
  writePending,
  editorBlocked,
  pendingAction,
  onNew,
  onRefresh,
  onSelect,
  onSelectProtocol,
  onToggle,
  onToggleProtocol,
}: RulesListPanelProps) {
  const protocolRows = [
    ...bodyRules.map((rule) => ({
      kind: "body" as const,
      label: "HTTP Body",
      listenerName: bodyListenerNames.get(rule.listener_id),
      rule,
    })),
    ...socketRules.map((rule) => ({
      kind: "socket" as const,
      label: "Socket",
      listenerName: socketListenerNames.get(rule.listener_id),
      rule,
    })),
  ];
  return (
    <div className="min-w-0 space-y-4 overflow-auto p-5">
      <div className="flex items-center gap-3">
        <div>
          <h2 className="text-lg font-semibold">规则</h2>
          <p className="text-sm text-[var(--telemetry-muted)]">
            规则按优先级数值从小到大逐条匹配；同优先级按创建顺序执行。
          </p>
        </div>
        <Button
          className="ml-auto"
          variant="primary"
          // 无效草稿只应阻止保存，不能阻止用户放弃草稿并重新开始。
          isDisabled={writePending}
          onPress={onNew}
        >
          <Plus className="size-4" />
          {pendingAction === "new" ? "正在新建…" : "新建规则"}
        </Button>
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
              if (kind === "body" || kind === "socket") {
                onSelectProtocol?.(kind, ruleId);
              } else {
                onSelect(ruleId);
              }
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
                      : "暂无规则，请选择新建规则开始配置"}
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
                    <div className="flex items-center gap-2">
                      <Chip size="sm" color="accent" variant="soft">HTTP</Chip>
                      <span>{rule.channel_text}</span>
                    </div>
                  </Table.Cell>
                  <Table.Cell>{rule.stage_text}</Table.Cell>
                  <Table.Cell>{rule.match_summary}</Table.Cell>
                  <Table.Cell>{rule.action_summary}</Table.Cell>
                  <Table.Cell>{rule.hit_count}</Table.Cell>
                  <Table.Cell>{formatTimestamp(rule.last_hit_at)}</Table.Cell>
                </Table.Row>
              ))}
              {protocolRows.map(({ kind, label, listenerName, rule }) => (
                <Table.Row
                  key={`${kind}:${rule.rule_id}`}
                  id={`${kind}:${rule.rule_id}`}
                >
                  <Table.Cell>
                    <Switch
                      aria-label={`${rule.enabled ? "停用" : "启用"} ${kind === "body" ? "Body" : "Socket"} 报文规则 ${rule.name}`}
                      isSelected={rule.enabled}
                      isDisabled={writePending || editorBlocked}
                      onChange={(enabled) => onToggleProtocol?.(kind, rule, enabled)}
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
                      <Chip size="sm" color="accent" variant="soft">{label}</Chip>
                      <span>{listenerName ?? "—"}</span>
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
    </div>
  );
}
