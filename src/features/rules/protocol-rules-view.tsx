"use client";

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { toast } from "@heroui/react";
import type {
  ProxyWorkspace,
  ProtocolDocumentRuleDefinition,
  ProtocolRuleCapabilityCatalog,
  ProtocolRuleStage,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ProtocolRuleEditor } from "./protocol-rule-editor";
import {
  capabilityCompatible,
  deleteResponseMatches,
  draftFromRule,
  isProtocolRuleList,
  listenerStages,
  newProtocolRuleDraft,
  protocolRulePackage,
  saveResponseMatches,
  protocolRuleListeners,
  type ProtocolRuleKind,
  type ProtocolRuleDraft,
  validateCapabilityCatalog,
  validateProtocolRuleDraft,
  toggleResponseMatches,
} from "./protocol-rule-model";
import { ProtocolRulesList } from "./protocol-rules-list";

export function ProtocolRulesView({ kind }: { kind: ProtocolRuleKind }) {
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>("protocol-rule-workspaces", () => callCommand(commands.workspaceList()));
  const workspaceId = workspaces.data?.find((workspace) => workspace.selected)?.id;
  const workspace = useIpcQuery<ProxyWorkspace>(
    `protocol-rule-workspace:${workspaceId ?? "none"}`,
    () => callCommand(commands.workspaceGet(workspaceId!)),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const rules = useIpcQuery<ProtocolDocumentRuleDefinition[]>(`protocol-rule-list:${workspaceId ?? "none"}`, () => callCommand(commands.protocolRuleList()), undefined, { enabled: Boolean(workspaceId) });
  const listeners = useMemo(() => protocolRuleListeners(
    Array.isArray(workspace.data?.listeners) ? workspace.data.listeners : [],
    kind,
  ), [kind, workspace.data]);
  const ruleListValid = rules.data === undefined || isProtocolRuleList(rules.data);
  const listenerIds = useMemo(
    () => new Set(listeners.map((listener) => listener.id)),
    [listeners],
  );
  const safeRules = useMemo(
    () => rules.data !== undefined && ruleListValid
      ? rules.data.filter((rule) => listenerIds.has(rule.listener_id))
      : [],
    [listenerIds, ruleListValid, rules.data],
  );
  const rulePayloadError = !ruleListValid
    ? "报文规则列表包含无效数据，已拒绝显示。"
    : undefined;
  const sourceLoading = workspaces.isLoading || workspace.isLoading || rules.isLoading;
  const combinedListError = workspaces.error ?? workspace.error ?? rules.error ?? rulePayloadError;
  const sourceBlocked = sourceLoading || Boolean(combinedListError);
  const [selectedId, setSelectedId] = useState<string>();
  const [creating, setCreating] = useState(false);
  const [listenerId, setListenerId] = useState<string>();
  const [stage, setStage] = useState<ProtocolRuleStage>("app_to_proxy");
  const [draft, setDraft] = useState<ProtocolRuleDraft>();
  const [editorWorkspaceId, setEditorWorkspaceId] = useState<string>();
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const [pending, setPending] = useState(false);
  const [valueStates, setValueStates] = useState<Record<string, { pending: boolean; invalid: boolean }>>({});
  const editorHeadingRef = useRef<HTMLDivElement>(null);
  const mutationLock = useRef(false);
  const editorGeneration = useRef(0);
  const editorContextCurrent = Boolean(workspaceId && editorWorkspaceId === workspaceId);
  const activeListenerId = editorContextCurrent ? listenerId : undefined;
  const selectedListener = listeners.find((listener) => listener.id === activeListenerId);
  const capabilities = useIpcQuery<ProtocolRuleCapabilityCatalog>(
    `protocol-rule-capabilities:${activeListenerId ?? "none"}:${stage}`,
    () => callCommand(commands.protocolRuleCapabilities(activeListenerId!, stage)),
    undefined,
    { enabled: Boolean(activeListenerId) },
  );
  const refreshWorkspaces = workspaces.refresh;
  const refreshWorkspace = workspace.refresh;
  const refreshRules = rules.refresh;
  const refreshCapabilities = capabilities.refresh;
  const refreshSocketContext = useCallback(async () => {
    // 外部窗口可能同时修改 Workspace、Listener 或规则。保留本地草稿供用户处理
    // revision 冲突，但刷新其全部事实来源，使不兼容绑定立即进入 fail-closed 状态。
    await Promise.all([
      refreshWorkspaces(),
      refreshWorkspace(),
      refreshRules(),
      refreshCapabilities(),
    ]);
  }, [refreshCapabilities, refreshRules, refreshWorkspace, refreshWorkspaces]);
  useAppEventRefresh(["workspace_changed", "snapshot_required"], refreshSocketContext);

  const receivedCatalogValidation = capabilities.data !== undefined
    ? validateCapabilityCatalog(capabilities.data)
    : undefined;
  const usableCatalog = !receivedCatalogValidation && capabilityMatchesSelection(
    capabilities.data,
    selectedListener,
    stage,
  ) ? capabilities.data : undefined;
  const bindingError = capabilities.data && !receivedCatalogValidation && !usableCatalog
    ? "规则能力与当前入口的协议版本或数据方向不一致。"
    : undefined;

  const preparedDraft = draft ?? (
    creating && selectedListener && usableCatalog
      ? newProtocolRuleDraft(selectedListener, stage, usableCatalog)
      : undefined
  );
  const valueParsing = Object.values(valueStates).some((state) => state.pending);
  const mutationContext = useMemo(() => ({
    workspaceId,
    editorWorkspaceId,
    selectedId,
    listenerId,
    stage,
    draft: preparedDraft,
    listener: selectedListener,
    catalog: usableCatalog,
  }), [editorWorkspaceId, listenerId, preparedDraft, selectedId, selectedListener, stage, usableCatalog, workspaceId]);
  const mutationContextRef = useRef(mutationContext);
  const mutationContextKey = JSON.stringify(mutationContext);
  useLayoutEffect(() => {
    if (JSON.stringify(mutationContextRef.current) !== mutationContextKey) {
      editorGeneration.current += 1;
      mutationContextRef.current = mutationContext;
    }
  }, [mutationContext, mutationContextKey]);

  useEffect(() => {
    if (!creating || !draft || !usableCatalog || capabilityCompatible(draft, usableCatalog)) return;
    // 外部刷新改变精确包、Schema 或能力目录时，旧的新建草稿不能继续提交。
    // 兼容草稿（包括编辑既有规则的 revision 冲突草稿）保持不变。
    editorGeneration.current += 1;
    const task = window.setTimeout(() => {
      setDraft(undefined);
      setFieldErrors({});
      setValueStates({});
    }, 0);
    return () => window.clearTimeout(task);
  }, [creating, draft, usableCatalog]);

  function chooseRule(rule: ProtocolDocumentRuleDefinition) {
    if (sourceBlocked || valueParsing) return;
    editorGeneration.current += 1;
    setCreating(false);
    setEditorWorkspaceId(workspaceId);
    setSelectedId(rule.rule_id);
    setListenerId(rule.listener_id);
    setStage(rule.stage);
    setDraft(draftFromRule(rule));
    resetDerivedState();
  }

  function newRule() {
    if (sourceBlocked || valueParsing) return;
    const listener = listeners[0];
    if (!listener) return;
    const nextStage = listenerStages(listener)[0];
    editorGeneration.current += 1;
    setCreating(true);
    setEditorWorkspaceId(workspaceId);
    setSelectedId(undefined);
    setListenerId(listener.id);
    setStage(nextStage);
    setDraft(undefined);
    resetDerivedState();
    requestAnimationFrame(() => editorHeadingRef.current?.focus());
  }

  function changeListener(nextId: string) {
    if (sourceBlocked || valueParsing) return;
    const listener = listeners.find((item) => item.id === nextId);
    if (!listener) return;
    editorGeneration.current += 1;
    setListenerId(nextId);
    setStage(listenerStages(listener)[0]);
    setDraft(undefined);
    resetDerivedState();
  }

  function changeStage(nextStage: ProtocolRuleStage) {
    if (sourceBlocked || valueParsing) return;
    editorGeneration.current += 1;
    setStage(nextStage);
    setDraft(undefined);
    resetDerivedState();
  }

  async function save() {
    if (sourceBlocked || !preparedDraft || mutationLock.current || Object.keys(valueStates).length > 0) return;
    const validationError = usableCatalog
      ? validateProtocolRuleDraft(preparedDraft, usableCatalog)
      : "规则配置尚未准备完成。";
    if (validationError) {
      setFieldErrors({ general: [validationError] });
      toast(validationError, { variant: "danger" });
      return;
    }
    mutationLock.current = true;
    const request = mutationRequest(editorGeneration.current, mutationContextRef.current);
    setPending(true);
    setFieldErrors({});
    try {
      const saved = await callCommand(commands.protocolRuleSave(preparedDraft));
      if (!mutationRequestCurrent(request, editorGeneration.current, mutationContextRef.current)) return;
      const previous = preparedDraft.rule_id == null
        ? undefined
        : safeRules.find((rule) => rule.rule_id === preparedDraft.rule_id);
      if (!saveResponseMatches(saved, preparedDraft, previous)) throw new Error("报文规则保存响应无效。");
      setCreating(false);
      setSelectedId(saved.rule_id);
      setListenerId(saved.listener_id);
      setStage(saved.stage);
      setDraft(draftFromRule(saved));
      await rules.refresh();
      toast("报文规则已保存。", { variant: "success" });
    } catch (reason) {
      const appError = appErrorViewModel(reason);
      const message = errorMessage(reason);
      const backendFields = appError?.field_errors ?? {};
      setFieldErrors(Object.keys(backendFields).length > 0 ? backendFields : { general: [message] });
      toast(message, { variant: "danger" });
    } finally {
      mutationLock.current = false;
      setPending(false);
    }
  }

  async function toggle(rule: ProtocolDocumentRuleDefinition, enabled: boolean) {
    if (sourceBlocked || mutationLock.current || valueParsing) return;
    mutationLock.current = true;
    const request = mutationRequest(editorGeneration.current, mutationContextRef.current);
    setPending(true);
    try {
      const saved = await callCommand(commands.protocolRuleToggle(rule.rule_id, rule.revision, enabled));
      if (!mutationRequestCurrent(request, editorGeneration.current, mutationContextRef.current)) return;
      if (!toggleResponseMatches(saved, rule, enabled)) throw new Error("报文规则启停响应无效。");
      if (selectedId === saved.rule_id) setDraft(draftFromRule(saved));
      await rules.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      mutationLock.current = false;
      setPending(false);
    }
  }

  async function remove() {
    if (sourceBlocked || !draft?.rule_id || draft.expected_revision == null || mutationLock.current || valueParsing) return;
    mutationLock.current = true;
    const request = mutationRequest(editorGeneration.current, mutationContextRef.current);
    setPending(true);
    try {
      const result = await callCommand(commands.protocolRuleDelete(draft.rule_id, draft.expected_revision, true));
      if (!mutationRequestCurrent(request, editorGeneration.current, mutationContextRef.current)) return;
      if (!deleteResponseMatches(result, draft.rule_id)) throw new Error("报文规则删除响应无效。");
      setSelectedId(undefined);
      setListenerId(undefined);
      setDraft(undefined);
      await rules.refresh();
      editorHeadingRef.current?.focus();
    } catch (reason) {
      setFieldErrors(appErrorViewModel(reason)?.field_errors ?? { general: [errorMessage(reason)] });
    } finally {
      mutationLock.current = false;
      setPending(false);
    }
  }

  async function reloadSelectedRule() {
    if (sourceBlocked || !selectedId || mutationLock.current || valueParsing) return;
    mutationLock.current = true;
    setPending(true);
    try {
      const latest = await callCommand(commands.protocolRuleList());
      if (!isProtocolRuleList(latest)) throw new Error("协议规则列表响应无效");
      const selected = latest.find((rule) => rule.rule_id === selectedId);
      if (!selected) throw new Error("missing rule");
      chooseRule(selected);
      await rules.refresh();
    } catch (reason) {
      setFieldErrors({ general: [errorMessage(reason)] });
    } finally {
      mutationLock.current = false;
      setPending(false);
    }
  }

  function resetDerivedState() {
    setFieldErrors({});
    setValueStates({});
  }

  const effectiveDraft = !sourceBlocked && editorContextCurrent ? preparedDraft : undefined;
  const editingListener = listeners.find((listener) => listener.id === effectiveDraft?.listener_id) ?? selectedListener;
  const draftValidation = effectiveDraft && usableCatalog
    ? validateProtocolRuleDraft(effectiveDraft, usableCatalog)
    : undefined;
  const editorError = capabilities.error ?? receivedCatalogValidation ?? bindingError;
  return (
    <div className="grid h-full grid-cols-[minmax(520px,1fr)_620px] max-[1280px]:h-auto max-[1280px]:grid-cols-1">
      <ProtocolRulesList
        kind={kind}
        error={combinedListError}
        listeners={listeners}
        loading={sourceLoading}
        onNew={newRule}
        onRetry={() => { void workspaces.refresh(); void workspace.refresh(); void rules.refresh(); }}
        onSelect={chooseRule}
        onToggle={(rule, enabled) => void toggle(rule, enabled)}
        pending={pending || valueParsing || sourceBlocked}
        sideEffectsDisabled={pending || valueParsing || sourceBlocked}
        rules={sourceBlocked ? [] : safeRules}
        selectedId={editorContextCurrent ? selectedId : undefined}
      />
      <div aria-label="报文规则编辑区" ref={editorHeadingRef} role="region" tabIndex={-1}>
        <ProtocolRuleEditor
          blocked={sourceBlocked}
          catalog={editorError ? undefined : usableCatalog}
          creating={creating}
          draft={effectiveDraft}
          error={editorError}
          fieldErrors={fieldErrors}
          listener={editingListener}
          listeners={listeners}
          loading={Boolean(listenerId) && capabilities.isLoading}
          onChange={(next) => { editorGeneration.current += 1; setDraft(next); setFieldErrors({}); }}
          onDelete={() => void remove()}
          onStageChange={changeStage}
          onListenerChange={changeListener}
          onReload={() => void capabilities.refresh()}
          onReloadRule={() => void reloadSelectedRule()}
          onResetInvalidValues={() => setValueStates({})}
          onSave={() => void save()}
          pending={pending}
          validationError={draftValidation}
          valueStates={valueStates}
          onValueStateChange={(key, state) => setValueStates((current) => {
            const next = { ...current };
            if (state) next[key] = state; else delete next[key];
            return next;
          })}
        />
      </div>
    </div>
  );
}


type MutationContext = {
  workspaceId?: string;
  editorWorkspaceId?: string;
  selectedId?: string;
  listenerId?: string;
  stage: ProtocolRuleStage;
  draft?: ProtocolRuleDraft;
  listener?: ProxyWorkspace["listeners"][number];
  catalog?: ProtocolRuleCapabilityCatalog;
};

function mutationRequest(generation: number, context: MutationContext) {
  return { generation, context: JSON.stringify(context) };
}

function mutationRequestCurrent(
  request: ReturnType<typeof mutationRequest>,
  generation: number,
  context: MutationContext,
) {
  return request.generation === generation && request.context === JSON.stringify(context);
}

function capabilityMatchesSelection(
  catalog: ProtocolRuleCapabilityCatalog | undefined,
  listener: ProxyWorkspace["listeners"][number] | undefined,
  stage: ProtocolRuleStage,
) {
  if (!catalog || !listener || catalog.stage !== stage) return false;
  const packageRef = protocolRulePackage(listener);
  if (!packageRef) return false;
  return catalog.package.id === packageRef.id && catalog.package.version === packageRef.version;
}
