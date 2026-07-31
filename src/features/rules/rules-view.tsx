"use client";

/**
 * 拦截规则的列表与编辑工作台。
 *
 * 左侧列表来自 Rust，右侧 draft 是尚未保存的用户输入。规则匹配、优先级执行、
 * 第 N 次命中、终止动作、revision 冲突和持久化全部由 Rust 负责。前端只调用
 * Rust 提供的草稿/解析/保存命令并显示字段错误。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Chip,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  Spinner,
  Switch,
  Table,
  Tabs,
  TextArea,
  TextField,
  toast,
} from "@heroui/react";
import { Copy, FileArrowUp, FileArrowRightOut, Plus, TrashBin } from "@gravity-ui/icons";
import type {
  RuleDraft,
  RuleSummaryViewModel,
  RuleViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import {
  appErrorViewModel,
  callCommand,
  errorMessage,
} from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { formatTimestamp, toneColor } from "@/lib/format";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import {
  ActionsEditor,
  ConditionsEditor,
  type RuleDraftChange,
} from "./rule-editor";

export function RulesView() {
  const { bootstrap } = useBootstrap();
  const channelCatalog = bootstrap?.channel_catalog ?? [];
  const { navigate, searchParams } = useWorkspaceNavigation();
  const sourceSessionId = searchParams.get("sessionId");
  const rules = useIpcQuery<RuleSummaryViewModel[]>("rule-list", () =>
    callCommand(commands.ruleList()),
  );
  useAppEventRefresh(["rule_hit", "snapshot_required"], rules.refresh);
  const [selectedId, setSelectedId] = useState<string | "new">();
  const [draft, setDraft] = useState<RuleDraft>();
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const [pendingAction, setPendingAction] = useState<
    "new" | "import" | "export" | "save" | "copy" | `toggle:${string}`
  >();
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deletePending, setDeletePending] = useState(false);
  const [editorAsyncStates, setEditorAsyncStates] = useState<
    Record<string, { pending: boolean; invalid: boolean }>
  >({});
  const editorPanelRef = useRef<HTMLElement>(null);

  function revealEditor() {
    // 窄窗口中列表和编辑器上下排列，选择后滚到编辑区，避免用户误以为没响应。
    if (window.matchMedia("(max-width: 1280px)").matches) {
      requestAnimationFrame(() => {
        editorPanelRef.current?.scrollIntoView({ block: "start" });
      });
    }
  }

  const effectiveSelectedId =
    // 第一次进入时默认选中 Rust 列表中的第一条；“new”明确表示本地新草稿。
    selectedId === "new" ? undefined : (selectedId ?? rules.data?.[0]?.rule_id);
  const ruleDetail = useIpcQuery<RuleViewModel>(
    `rule-detail:${effectiveSelectedId ?? "none"}`,
    () => callCommand(commands.ruleGet(effectiveSelectedId!)),
    undefined,
    { enabled: Boolean(effectiveSelectedId) },
  );
  const writePending = pendingAction != null || deletePending;
  const editorBlocked = Object.values(editorAsyncStates).some(
    (state) => state.pending || state.invalid,
  );
  const fieldError = (field: string) => fieldErrors[field]?.join("；");
  const updateEditorAsyncState = useCallback(
    (
      key: string,
      state?: { pending: boolean; invalid: boolean },
    ) => {
      setEditorAsyncStates((current) => {
        const next = { ...current };
        if (state) next[key] = state;
        else delete next[key];
        return next;
      });
    },
    [],
  );

  function updateDraft(change: RuleDraftChange) {
    // 任意编辑都会清除上一次 Rust 字段错误，避免旧错误继续误导用户。
    setDraft((current) => {
      if (!current) return current;
      return typeof change === "function" ? change(current) : change;
    });
    setFieldErrors({});
  }

  useEffect(() => {
    // 详情异步到达后再装入草稿；timeout 避免在 effect 同步阶段级联更新。
    if (
      ruleDetail.data &&
      ruleDetail.data.summary.rule_id === effectiveSelectedId
    ) {
      const task = window.setTimeout(() => {
        setDraft(ruleDetail.data?.draft);
        setEditorAsyncStates({});
      }, 0);
      return () => window.clearTimeout(task);
    }
  }, [effectiveSelectedId, ruleDetail.data]);

  useEffect(() => {
    // 从抓包/会话进入时只携带 sessionId，Rust 负责生成合法的预填草稿。
    if (!sourceSessionId) return;
    let active = true;
    void callCommand(commands.ruleCreateFromSession(sourceSessionId))
      .then((value) => {
        if (!active) return;
        setDraft(value);
        setEditorAsyncStates({});
        setSelectedId("new");
        navigate("/rules");
      })
      .catch((reason) => {
        if (active) toast(errorMessage(reason), { variant: "danger" });
      });
    return () => {
      active = false;
    };
  }, [navigate, sourceSessionId]);

  async function newRule() {
    if (writePending || editorBlocked) return;
    setPendingAction("new");
    try {
      setDraft(await callCommand(commands.ruleNewDraft()));
      setEditorAsyncStates({});
      setSelectedId("new");
      revealEditor();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function transferRules(mode: "import" | "export") {
    if (writePending) return;
    setPendingAction(mode);
    try {
      const result = await callCommand(
        mode === "import" ? commands.ruleImport() : commands.ruleExport(),
      );
      toast(result.message, { variant: toneColor(result.ui_tone) });
      await rules.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function save() {
    // editorBlocked 表示某个异步草稿转换还未完成或失败，不能保存半成品。
    if (!draft || writePending || editorBlocked) return;
    setPendingAction("save");
    try {
      const result = await callCommand(commands.ruleSave(draft));
      setDraft(result.draft);
      setEditorAsyncStates({});
      setFieldErrors({});
      setSelectedId(result.summary.rule_id);
      toast(`规则“${result.summary.name}”已保存。`, { variant: "success" });
      await rules.refresh();
    } catch (reason) {
      setFieldErrors(appErrorViewModel(reason)?.field_errors ?? {});
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function toggle(rule: RuleSummaryViewModel, enabled: boolean) {
    // 启停也带 revision，防止两个窗口用旧版本互相覆盖。
    if (writePending) return;
    setPendingAction(`toggle:${rule.rule_id}`);
    try {
      const result = await callCommand(
        commands.ruleToggle(rule.rule_id, rule.revision, enabled),
      );
      if (result.summary.rule_id === effectiveSelectedId) {
        setDraft(result.draft);
      }
      await rules.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function copySelected() {
    if (!effectiveSelectedId || writePending || editorBlocked) return;
    setPendingAction("copy");
    try {
      const result = await callCommand(commands.ruleCopy(effectiveSelectedId));
      setSelectedId(result.summary.rule_id);
      setDraft(result.draft);
      setEditorAsyncStates({});
      await rules.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  async function remove() {
    if (writePending || editorBlocked) return;
    const selected = rules.data?.find(
      (item) => item.rule_id === effectiveSelectedId,
    );
    if (!selected) return;
    setDeletePending(true);
    try {
      const result = await callCommand(
        commands.ruleDelete(selected.rule_id, selected.revision, true),
      );
      toast(result.message, { variant: toneColor(result.ui_tone) });
      setSelectedId(undefined);
      setDraft(undefined);
      setEditorAsyncStates({});
      await rules.refresh();
      setDeleteDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setDeletePending(false);
    }
  }

  return (
    <section className="grid h-full grid-cols-[minmax(600px,1fr)_560px] max-[1280px]:grid-cols-1">
      <div className="min-w-0 space-y-5 overflow-auto p-5">
        <div className="flex items-center">
          <h1 className="text-2xl font-semibold">拦截规则</h1>
          <div className="ml-auto flex gap-3">
            <Button
              variant="primary"
              isDisabled={writePending || editorBlocked}
              onPress={() => void newRule()}
            >
              <Plus className="size-4" />
              {pendingAction === "new" ? "正在新建…" : "新建规则"}
            </Button>
            <Button
              variant="outline"
              isDisabled={writePending}
              onPress={() => void transferRules("import")}
            >
              <FileArrowUp className="size-4" />
              {pendingAction === "import" ? "正在导入…" : "导入规则"}
            </Button>
            <Button
              variant="outline"
              isDisabled={writePending}
              onPress={() => void transferRules("export")}
            >
              <FileArrowRightOut className="size-4" />
              {pendingAction === "export" ? "正在导出…" : "导出规则"}
            </Button>
          </div>
        </div>
        {rules.error && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>规则列表读取失败</Alert.Title>
              <Alert.Description>{rules.error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void rules.refresh()}
            >
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
                selectedKeys={effectiveSelectedId ? [effectiveSelectedId] : []}
              onSelectionChange={(keys) => {
                if (keys === "all") return;
                const first = Array.from(keys)[0];
                if (first != null) {
                  setDraft(undefined);
                  setSelectedId(String(first));
                  revealEditor();
                }
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
                    {rules.isLoading
                      ? "正在读取规则…"
                      : rules.error
                        ? "规则列表暂不可用"
                        : "暂无拦截规则"}
                  </div>
                )}
              >
                {(rules.data ?? []).map((rule) => (
                  <Table.Row key={rule.rule_id} id={rule.rule_id}>
                    <Table.Cell>
                      <Switch
                        aria-label={`${rule.enabled ? "停用" : "启用"}规则 ${rule.name}`}
                        isSelected={rule.enabled}
                        isDisabled={writePending || editorBlocked}
                        onChange={(enabled) => void toggle(rule, enabled)}
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
              Rust 按优先级升序、同优先级按创建顺序执行；命中终止动作后停止后续规则。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      </div>

      <aside
        ref={editorPanelRef}
        className="scroll-mt-4 overflow-auto border-l border-[var(--telemetry-line)] p-5 [scrollbar-gutter:stable] max-[1280px]:border-l-0 max-[1280px]:border-t"
      >
        <h2 className="mb-4 text-lg font-semibold">
          {draft
            ? `编辑规则：${draft.name}`
            : ruleDetail.isLoading
              ? "正在读取规则…"
              : "选择规则进行编辑"}
        </h2>
        {ruleDetail.isLoading && !draft && (
          <div className="grid min-h-40 place-items-center">
            <Spinner aria-label="正在读取规则详情" />
          </div>
        )}
        {ruleDetail.error && !draft && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>规则详情读取失败</Alert.Title>
              <Alert.Description>{ruleDetail.error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void ruleDetail.refresh()}
            >
              重试
            </Button>
          </Alert>
        )}
        {draft && (
          <Form className="space-y-4">
            <Tabs defaultSelectedKey="basic">
              <Tabs.ListContainer>
                <Tabs.List aria-label="规则编辑">
                  <Tabs.Tab id="basic">
                    基本信息
                    <Tabs.Indicator />
                  </Tabs.Tab>
                  <Tabs.Tab id="conditions">
                    匹配条件
                    <Tabs.Indicator />
                  </Tabs.Tab>
                  <Tabs.Tab id="actions">
                    执行动作
                    <Tabs.Indicator />
                  </Tabs.Tab>
                </Tabs.List>
              </Tabs.ListContainer>
              <Tabs.Panel id="basic" className="space-y-4 pt-4">
                <TextField isInvalid={Boolean(fieldError("name"))}>
                  <Label>规则名称</Label>
                  <Input
                    value={draft.name}
                    onChange={(event) =>
                      updateDraft({ ...draft, name: event.target.value })
                    }
                  />
                  {fieldError("name") && (
                    <FieldError>{fieldError("name")}</FieldError>
                  )}
                </TextField>
                <TextField>
                  <Label>规则说明</Label>
                  <TextArea
                    value={draft.description}
                    onChange={(event) =>
                      updateDraft({
                        ...draft,
                        description: event.target.value,
                      })
                    }
                  />
                </TextField>
                <NumberField
                  value={draft.priority}
                  onChange={(value) =>
                    updateDraft({ ...draft, priority: value })
                  }
                >
                  <Label>规则优先级</Label>
                  <NumberField.Group className="w-full">
                    <NumberField.DecrementButton />
                    <NumberField.Input />
                    <NumberField.IncrementButton />
                  </NumberField.Group>
                </NumberField>
                <div className="grid gap-1">
                  <Label>通道</Label>
                  <Select
                    aria-label="规则通道"
                    selectedKey={draft.channel ?? "all"}
                    onSelectionChange={(key) =>
                      updateDraft({
                        ...draft,
                        channel:
                          key === "all"
                            ? null
                            : (key as RuleDraft["channel"]),
                      })
                    }
                  >
                    <Select.Trigger>
                      <Select.Value />
                      <Select.Indicator />
                    </Select.Trigger>
                    <Select.Popover>
                      <ListBox>
                        <ListBox.Item id="all" textValue="全部">
                          全部
                        </ListBox.Item>
                        {channelCatalog.map((channel) => (
                          <ListBox.Item
                            key={channel.id}
                            id={channel.id}
                            textValue={channel.display_name}
                          >
                            {channel.display_name}
                          </ListBox.Item>
                        ))}
                      </ListBox>
                    </Select.Popover>
                  </Select>
                </div>
                <div className="grid gap-1">
                  <Label>阶段</Label>
                  <Select
                    aria-label="规则阶段"
                    selectedKey={draft.stage ?? "none"}
                    onSelectionChange={(key) =>
                      updateDraft({
                        ...draft,
                        stage:
                          key === "none" ? null : (key as RuleDraft["stage"]),
                      })
                    }
                  >
                    <Select.Trigger>
                      <Select.Value />
                      <Select.Indicator />
                    </Select.Trigger>
                    <Select.Popover>
                      <ListBox>
                        <ListBox.Item id="none">请选择</ListBox.Item>
                        <ListBox.Item id="tls_handshake">TLS 握手</ListBox.Item>
                        <ListBox.Item id="request">请求</ListBox.Item>
                        <ListBox.Item id="response">响应</ListBox.Item>
                      </ListBox>
                    </Select.Popover>
                  </Select>
                </div>
                <div className="flex flex-wrap gap-5">
                  <Switch
                    aria-label="启用规则"
                    isSelected={draft.enabled}
                    onChange={(enabled) => updateDraft({ ...draft, enabled })}
                  >
                    <Switch.Content>
                      <Switch.Control>
                        <Switch.Thumb />
                      </Switch.Control>
                      <span>启用规则</span>
                    </Switch.Content>
                  </Switch>
                  <Switch
                    aria-label="仅命中一次"
                    isSelected={draft.one_shot}
                    onChange={(one_shot) =>
                      updateDraft({ ...draft, one_shot })
                    }
                  >
                    <Switch.Content>
                      <Switch.Control>
                        <Switch.Thumb />
                      </Switch.Control>
                      <span>仅命中一次</span>
                    </Switch.Content>
                  </Switch>
                </div>
              </Tabs.Panel>
              <Tabs.Panel id="conditions" className="pt-4">
                <ConditionsEditor
                  draft={draft}
                  fieldErrors={fieldErrors}
                  onChange={updateDraft}
                  onAsyncStateChange={updateEditorAsyncState}
                />
                <p className="mt-2 text-xs text-[var(--telemetry-muted)]">
                  空条件表示匹配该通道和阶段的全部消息；保存时由 Rust 统一校验。
                </p>
              </Tabs.Panel>
              <Tabs.Panel id="actions" className="pt-4">
                <ActionsEditor
                  draft={draft}
                  fieldErrors={fieldErrors}
                  onChange={updateDraft}
                  onAsyncStateChange={updateEditorAsyncState}
                />
                <p className="mt-2 text-xs text-[var(--telemetry-muted)]">
                  动作顺序即执行顺序，终止动作会中断后续评估。
                </p>
              </Tabs.Panel>
            </Tabs>
            <Alert status="success">
              配置将由 Rust 校验字段、正则、JSON 路径和动作兼容性。
            </Alert>
            {Object.keys(fieldErrors).length > 0 && (
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>规则配置校验失败</Alert.Title>
                  <Alert.Description>
                    {Object.values(fieldErrors).flat().join("；")}
                  </Alert.Description>
                </Alert.Content>
              </Alert>
            )}
            <div className="flex gap-3">
              <Button
                variant="primary"
                isDisabled={writePending || editorBlocked}
                onPress={() => void save()}
              >
                {pendingAction === "save"
                  ? "正在保存…"
                  : editorBlocked
                    ? "等待 Rust 解析输入"
                    : "保存规则"}
              </Button>
              <Button
                variant="outline"
                isDisabled={!effectiveSelectedId || writePending || editorBlocked}
                onPress={() => void copySelected()}
              >
                <Copy className="size-4" />
                {pendingAction === "copy" ? "正在复制…" : "复制规则"}
              </Button>
              <AlertDialog
                isOpen={deleteDialogOpen}
                onOpenChange={(open) => {
                  if (!open && deletePending) return;
                  setDeleteDialogOpen(open);
                }}
              >
                <Button
                  variant="danger-soft"
                  isDisabled={!effectiveSelectedId || writePending || editorBlocked}
                >
                  <TrashBin className="size-4" />
                  删除规则
                </Button>
                <AlertDialog.Backdrop>
                  <AlertDialog.Container>
                    <AlertDialog.Dialog>
                      <AlertDialog.Header>
                        <AlertDialog.Heading>删除此规则？</AlertDialog.Heading>
                      </AlertDialog.Header>
                      <AlertDialog.Body>删除后无法恢复。</AlertDialog.Body>
                      <AlertDialog.Footer>
                        <Button
                          slot="close"
                          variant="outline"
                          isDisabled={deletePending}
                        >
                          取消
                        </Button>
                        <Button
                          variant="danger"
                          isDisabled={deletePending}
                          onPress={() => void remove()}
                        >
                          {deletePending ? "正在删除…" : "确认删除"}
                        </Button>
                      </AlertDialog.Footer>
                    </AlertDialog.Dialog>
                  </AlertDialog.Container>
                </AlertDialog.Backdrop>
              </AlertDialog>
            </div>
          </Form>
        )}
      </aside>
    </section>
  );
}
