"use client";

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { toast } from "@heroui/react";
import type {
  ProxyWorkspace,
  ProtocolDocumentRuleDefinition,
  ProtocolRuleCapabilityCatalog,
  ProtocolRuleStage,
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
  type ProtocolRuleKind,
  type ProtocolRuleDraft,
  validateCapabilityCatalog,
  validateProtocolRuleDraft,
  toggleResponseMatches,
} from "./protocol-rule-model";
import { ProtocolRulesList } from "./protocol-rules-list";
import { RulesWorkspaceShell } from "./rules-workspace-shell";
import {
  type ProtocolRuleSource,
  useProtocolRuleSource,
} from "./use-protocol-rule-source";

type ProtocolRulesViewProps = {
  kind: ProtocolRuleKind;
  selectedRuleId?: string;
  createOnMount?: boolean;
  onCreateHandled?: () => void;
  onChanged?: (ruleId?: string) => void;
  onPendingChange?: (pending: boolean) => void;
};

export function ProtocolRulesView({
  kind,
  ...props
}: ProtocolRulesViewProps) {
  const source = useProtocolRuleSource(kind);
  return <ProtocolRulesController kind={kind} source={source} {...props} />;
}

export function ProtocolRuleEditorView({
  source,
  ...props
}: Omit<ProtocolRulesViewProps, "kind"> & { source: ProtocolRuleSource }) {
  return <ProtocolRulesController kind="http" source={source} editorOnly {...props} />;
}

function ProtocolRulesController({
  kind,
  source,
  editorOnly = false,
  selectedRuleId,
  createOnMount = false,
  onCreateHandled,
  onChanged,
  onPendingChange,
}: ProtocolRulesViewProps & {
  source: ProtocolRuleSource;
  editorOnly?: boolean;
}) {
  const { workspaceId, listeners, rules: safeRules } = source;
  const sourceLoading = source.isLoading;
  const combinedListError = source.error;
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
  const createHandled = useRef(false);
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
  const refreshCapabilities = capabilities.refresh;
  useAppEventRefresh(["workspace_changed", "snapshot_required"], refreshCapabilities);

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
  useEffect(() => {
    onPendingChange?.(pending || valueParsing || sourceBlocked);
  }, [onPendingChange, pending, sourceBlocked, valueParsing]);
  useEffect(
    () => () => onPendingChange?.(false),
    [onPendingChange],
  );
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

  function resetDerivedState() {
    setFieldErrors({});
    setValueStates({});
  }

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

  const newRule = useCallback(() => {
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
  }, [listeners, sourceBlocked, valueParsing, workspaceId]);

  useEffect(() => {
    if (!createOnMount) {
      createHandled.current = false;
      return;
    }
    if (createHandled.current || sourceBlocked || listeners.length === 0) return;
    createHandled.current = true;
    newRule();
    onCreateHandled?.();
  }, [createOnMount, listeners.length, newRule, onCreateHandled, sourceBlocked]);

  useEffect(() => {
    if (!selectedRuleId || sourceBlocked || selectedId === selectedRuleId) return;
    const selected = safeRules.find((rule) => rule.rule_id === selectedRuleId);
    if (!selected) return;
    editorGeneration.current += 1;
    const task = window.setTimeout(() => {
      setCreating(false);
      setEditorWorkspaceId(workspaceId);
      setSelectedId(selected.rule_id);
      setListenerId(selected.listener_id);
      setStage(selected.stage);
      setDraft(draftFromRule(selected));
      resetDerivedState();
    }, 0);
    return () => window.clearTimeout(task);
  }, [safeRules, selectedId, selectedRuleId, sourceBlocked, workspaceId]);

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
      await source.refresh();
      onChanged?.(saved.rule_id);
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
      await source.refresh();
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
      await source.refresh();
      onChanged?.();
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
      await source.refresh();
    } catch (reason) {
      setFieldErrors({ general: [errorMessage(reason)] });
    } finally {
      mutationLock.current = false;
      setPending(false);
    }
  }

  const effectiveDraft = !sourceBlocked && editorContextCurrent ? preparedDraft : undefined;
  const editingListener = listeners.find((listener) => listener.id === effectiveDraft?.listener_id) ?? selectedListener;
  const draftValidation = effectiveDraft && usableCatalog
    ? validateProtocolRuleDraft(effectiveDraft, usableCatalog)
    : undefined;
  const editorError = capabilities.error ?? receivedCatalogValidation ?? bindingError;
  const editor = (
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
  );
  if (editorOnly) return editor;
  return (
    <RulesWorkspaceShell>
      <ProtocolRulesList
        kind={kind}
        error={combinedListError}
        listeners={listeners}
        loading={sourceLoading}
        onNew={newRule}
        onRetry={() => void source.refresh()}
        onSelect={chooseRule}
        onToggle={(rule, enabled) => void toggle(rule, enabled)}
        pending={pending || valueParsing || sourceBlocked}
        sideEffectsDisabled={pending || valueParsing || sourceBlocked}
        rules={sourceBlocked ? [] : safeRules}
        selectedId={editorContextCurrent ? selectedId : undefined}
      />
      {editor}
    </RulesWorkspaceShell>
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
