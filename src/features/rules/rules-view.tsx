"use client";

/**
 * 拦截规则的列表与编辑工作台。
 *
 * 左侧列表来自 Rust，右侧 draft 是尚未保存的用户输入。规则匹配、优先级执行、
 * 第 N 次命中、终止动作、revision 冲突和持久化全部由 Rust 负责。前端只调用
 * Rust 提供的草稿/解析/保存命令并显示字段错误。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "@heroui/react";
import type {
  ProtocolDocumentRuleDefinition,
  RuleDraft,
  RuleStageCapabilityViewModel,
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
import { ProtocolRuleEditorView } from "./protocol-rules-view";
import { RulesWorkspaceShell } from "./rules-workspace-shell";
import { RuleCreationDialogs } from "./rule-creation-dialogs";
import { useProtocolRuleSource } from "./use-protocol-rule-source";
import { toggleResponseMatches } from "./protocol-rule-model";

export function RulesView() {
  return (
    <div
      aria-label="统一规则工作区"
      className="h-full min-h-0 overflow-auto p-3"
    >
      <div className="h-full min-h-[42rem] max-[1280px]:h-auto">
        <HttpRulesView />
      </div>
    </div>
  );
}

function HttpRulesView() {
  const { navigate, searchParams } = useWorkspaceNavigation();
  const category = searchParams.get("category");
  const protocolEditor = category === "body" || category === "socket"
    ? category
    : undefined;
  return (
    <HttpStandardRulesView
      protocolEditor={protocolEditor}
      protocolRuleId={searchParams.get("ruleId") ?? undefined}
      createProtocolOnMount={Boolean(protocolEditor) && searchParams.get("create") === "rule"}
      onProtocolCreateHandled={() => navigate(`/rules?category=${protocolEditor}`)}
    />
  );
}

function HttpStandardRulesView({
  protocolEditor,
  protocolRuleId,
  createProtocolOnMount,
  onProtocolCreateHandled,
}: {
  protocolEditor?: "body" | "socket";
  protocolRuleId?: string;
  createProtocolOnMount: boolean;
  onProtocolCreateHandled: () => void;
}) {
  const { bootstrap } = useBootstrap();
  const channelCatalog = bootstrap?.channel_catalog ?? [];
  const { navigate, searchParams } = useWorkspaceNavigation();
  const sourceSessionId = searchParams.get("sessionId");
  const requestedCreate = searchParams.get("create");
  const rules = useIpcQuery<RuleSummaryViewModel[]>("rule-list", () =>
    callCommand(commands.ruleList()),
  );
  const capabilities = useIpcQuery<RuleStageCapabilityViewModel[]>(
    "rule-capabilities",
    () => callCommand(commands.ruleCapabilities()),
  );
  const bodySource = useProtocolRuleSource("http");
  const socketSource = useProtocolRuleSource("socket");
  const bodyListenerNames = useMemo(
    () => new Map(bodySource.listeners.map((listener) => [listener.id, listener.name])),
    [bodySource.listeners],
  );
  const socketListenerNames = useMemo(
    () => new Map(socketSource.listeners.map((listener) => [listener.id, listener.name])),
    [socketSource.listeners],
  );
  useAppEventRefresh(["rule_hit", "snapshot_required"], rules.refresh);
  const [selectedId, setSelectedId] = useState<string | "new">();
  const [draft, setDraft] = useState<RuleDraft>();
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const [pendingAction, setPendingAction] = useState<
    "new" | "save" | "copy" | `toggle:${string}`
  >();
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deletePending, setDeletePending] = useState(false);
  const [creationChoiceOpen, setCreationChoiceOpen] = useState(false);
  const [faultPresetOpen, setFaultPresetOpen] = useState(
    requestedCreate === "fault",
  );
  const [protocolEditorPending, setProtocolEditorPending] = useState(false);
  const [editorAsyncStates, setEditorAsyncStates] = useState<
    Record<string, { pending: boolean; invalid: boolean }>
  >({});
  const editorPanelRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (requestedCreate !== "fault") return;
    const task = window.setTimeout(() => {
      setFaultPresetOpen(true);
      navigate("/rules");
    }, 0);
    return () => window.clearTimeout(task);
  }, [navigate, requestedCreate]);

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
  const writePending = pendingAction != null || deletePending || protocolEditorPending;
  const editorBlocked = Object.values(editorAsyncStates).some(
    (state) => state.pending || state.invalid,
  ) || capabilities.error != null || capabilities.data == null;
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
    // 从抓包进入时只携带内部 sessionId，Rust 负责生成合法的预填草稿。
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
    // 新建会替换当前草稿，因此当前草稿无效不应把用户锁在编辑器里。
    if (writePending) return;
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
    // 启停也带 revision，防止两个窗口用陈旧快照互相覆盖。
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

  async function toggleProtocol(
    kind: "body" | "socket",
    rule: ProtocolDocumentRuleDefinition,
    enabled: boolean,
  ) {
    if (writePending) return;
    setPendingAction(`toggle:${rule.rule_id}`);
    try {
      const saved = await callCommand(
        commands.protocolRuleToggle(rule.rule_id, rule.revision, enabled),
      );
      if (!toggleResponseMatches(saved, rule, enabled)) {
        throw new Error("报文规则启停响应无效。");
      }
      await (kind === "body" ? bodySource : socketSource).refresh();
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

  const activeProtocolSource = protocolEditor === "body"
    ? bodySource
    : protocolEditor === "socket"
      ? socketSource
      : undefined;

  return (
    <RulesWorkspaceShell>
      <RulesListPanel
        rules={rules.data}
        bodyRules={bodySource.rules}
        bodyListenerNames={bodyListenerNames}
        socketRules={socketSource.rules}
        socketListenerNames={socketListenerNames}
        error={rules.error ?? bodySource.error ?? socketSource.error}
        isLoading={rules.isLoading || bodySource.isLoading || socketSource.isLoading}
        selectedId={protocolEditor ? protocolRuleId : effectiveSelectedId}
        selectedKind={protocolEditor ?? "standard"}
        writePending={writePending}
        editorBlocked={editorBlocked}
        pendingAction={pendingAction}
        onNew={() => setCreationChoiceOpen(true)}
        onRefresh={() => void Promise.all([
          rules.refresh(),
          bodySource.refresh(),
          socketSource.refresh(),
        ])}
        onSelect={(ruleId) => {
          navigate("/rules");
          setDraft(undefined);
          setSelectedId(ruleId);
          revealEditor();
        }}
        onSelectProtocol={(kind, ruleId) => {
          navigate(`/rules?category=${kind}&ruleId=${encodeURIComponent(ruleId)}`);
          revealEditor();
        }}
        onToggle={(rule, enabled) => void toggle(rule, enabled)}
        onToggleProtocol={(kind, rule, enabled) => void toggleProtocol(kind, rule, enabled)}
      />
      {protocolEditor && activeProtocolSource ? (
        <ProtocolRuleEditorView
          kind={protocolEditor === "body" ? "http" : "socket"}
          source={activeProtocolSource}
          selectedRuleId={protocolRuleId}
          createOnMount={createProtocolOnMount}
          onCreateHandled={onProtocolCreateHandled}
          onPendingChange={setProtocolEditorPending}
          onChanged={(ruleId) => {
            void activeProtocolSource.refresh();
            navigate(
              ruleId
                ? `/rules?category=${protocolEditor}&ruleId=${encodeURIComponent(ruleId)}`
                : "/rules",
            );
          }}
        />
      ) : (
        <RuleEditorPanel
          panelRef={editorPanelRef}
          draft={draft}
          isLoading={ruleDetail.isLoading}
          loadError={ruleDetail.error}
          fieldErrors={fieldErrors}
          channelCatalog={channelCatalog}
          capabilities={capabilities.data}
          capabilityError={capabilities.error}
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
      )}
      <RuleCreationDialogs
        choiceOpen={creationChoiceOpen}
        faultPresetOpen={faultPresetOpen}
        onChoiceOpenChange={setCreationChoiceOpen}
        onFaultPresetOpenChange={setFaultPresetOpen}
        onBlankRule={() => {
          setCreationChoiceOpen(false);
          navigate("/rules");
          void newRule();
        }}
        onBodyRule={() => {
          setCreationChoiceOpen(false);
          setEditorAsyncStates({});
          navigate("/rules?category=body&create=rule");
        }}
        onSocketRule={() => {
          setCreationChoiceOpen(false);
          setEditorAsyncStates({});
          navigate("/rules?category=socket&create=rule");
        }}
        onFaultPreset={() => {
          setCreationChoiceOpen(false);
          setFaultPresetOpen(true);
        }}
        onRuleCreated={(ruleId) => {
          setFaultPresetOpen(false);
          setDraft(undefined);
          setSelectedId(ruleId);
          void rules.refresh();
          revealEditor();
        }}
      />
    </RulesWorkspaceShell>
  );
}
