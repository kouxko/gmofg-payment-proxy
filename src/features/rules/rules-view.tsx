"use client";

/**
 * 拦截规则的列表与编辑工作台。
 *
 * 左侧列表来自 Rust，右侧 draft 是尚未保存的用户输入。规则匹配、优先级执行、
 * 第 N 次命中、终止动作、revision 冲突和持久化全部由 Rust 负责。前端只调用
 * Rust 提供的草稿/解析/保存命令并显示字段错误。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Tabs, toast } from "@heroui/react";
import type {
  RuleDraft,
  RuleSummaryViewModel,
  RuleViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { toneColor } from "@/lib/format";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import type { RuleDraftChange } from "./rule-editor";
import { RuleEditorPanel } from "./rule-editor-panel";
import { RulesListPanel } from "./rules-list-panel";
import { SocketRulesView } from "./socket-rules-view";

export function RulesView() {
  const [mode, setMode] = useState<"http" | "socket">("http");
  return (
    <div className="flex h-full min-h-0 flex-col">
      <Tabs
        className="min-h-0 flex-1"
        onSelectionChange={(key) => setMode(key as "http" | "socket")}
        selectedKey={mode}
      >
        <Tabs.ListContainer className="border-b border-[var(--telemetry-line)] px-5 pt-3">
          <Tabs.List aria-label="规则类型">
            <Tabs.Tab id="http">HTTP 规则<Tabs.Indicator /></Tabs.Tab>
            <Tabs.Tab id="socket">Socket 规则<Tabs.Indicator /></Tabs.Tab>
          </Tabs.List>
        </Tabs.ListContainer>
        <Tabs.Panel className="h-full min-h-0" id={mode}>
          {mode === "http" ? <HttpRulesView /> : <SocketRulesView />}
        </Tabs.Panel>
      </Tabs>
    </div>
  );
}

function HttpRulesView() {
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
  const updateEditorAsyncState = useCallback(
    (key: string, state?: { pending: boolean; invalid: boolean }) => {
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
      <RulesListPanel
        rules={rules.data}
        error={rules.error}
        isLoading={rules.isLoading}
        selectedId={effectiveSelectedId}
        writePending={writePending}
        editorBlocked={editorBlocked}
        pendingAction={pendingAction}
        onNew={() => void newRule()}
        onImport={() => void transferRules("import")}
        onExport={() => void transferRules("export")}
        onRefresh={() => void rules.refresh()}
        onSelect={(ruleId) => {
          setDraft(undefined);
          setSelectedId(ruleId);
          revealEditor();
        }}
        onToggle={(rule, enabled) => void toggle(rule, enabled)}
      />
      <RuleEditorPanel
        panelRef={editorPanelRef}
        draft={draft}
        isLoading={ruleDetail.isLoading}
        loadError={ruleDetail.error}
        fieldErrors={fieldErrors}
        channelCatalog={channelCatalog}
        writePending={writePending}
        editorBlocked={editorBlocked}
        pendingAction={pendingAction}
        selectedId={effectiveSelectedId}
        deleteDialogOpen={deleteDialogOpen}
        deletePending={deletePending}
        onDraftChange={updateDraft}
        onAsyncStateChange={updateEditorAsyncState}
        onRetry={() => void ruleDetail.refresh()}
        onSave={() => void save()}
        onCopy={() => void copySelected()}
        onDelete={() => void remove()}
        onDeleteDialogChange={(open) => {
          if (!open && deletePending) return;
          setDeleteDialogOpen(open);
        }}
      />
    </section>
  );
}
