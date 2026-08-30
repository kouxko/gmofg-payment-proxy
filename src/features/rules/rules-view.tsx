"use client";

import { useEffect, useState } from "react";
import { toast } from "@heroui/react";
import type { RuleDefinitionSaveInput, RuleDefinition_Serialize, RuleEditorContext } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
import { RuleCreationDialog } from "./rule-creation-dialog";
import { RuleDefinitionEditor } from "./rule-definition-editor";
import { RuleDefinitionList } from "./rule-definition-list";
import { RulesWorkspaceShell } from "./rules-workspace-shell";
import { useRuleDefinitionSource } from "./use-rule-definition-source";

export function RulesView() {
  const source = useRuleDefinitionSource();
  const { navigate, searchParams } = useWorkspaceNavigation();
  const sourceExchangeId = searchParams.get("exchangeId");
  const sourceResponseEvent = searchParams.get("responseEvent");
  const [selected, setSelected] = useState<RuleDefinition_Serialize>();
  const [input, setInput] = useState<RuleDefinitionSaveInput>();
  const [context, setContext] = useState<RuleEditorContext>();
  const [loadingEditor, setLoadingEditor] = useState(false);
  const [pending, setPending] = useState(false);
  const [creationOpen, setCreationOpen] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const listener = source.listeners.find((item) => item.id === input?.draft.listener_id);
  useAppEventRefresh(["rule_hit", "snapshot_required"], source.refresh);

  useEffect(() => {
    if (!sourceExchangeId || sourceResponseEvent == null) return;
    const responseEventIndex = Number(sourceResponseEvent);
    if (!Number.isSafeInteger(responseEventIndex) || responseEventIndex < 0) {
      toast("抓包响应事件索引无效。", { variant: "danger" });
      navigate("/rules");
      return;
    }
    let active = true;
    void callCommand(commands.ruleDefinitionCreateFromExchangeObservation(sourceExchangeId, responseEventIndex))
      .then(async (draft) => {
        const editorContext = await callCommand(commands.ruleEditorContext(draft.draft.listener_id));
        if (!active) return;
        setSelected(undefined);
        setInput(draft);
        setContext(editorContext);
        setFieldErrors({});
        navigate("/rules");
      })
      .catch((reason) => {
        if (active) toast(errorMessage(reason), { variant: "danger" });
      });
    return () => { active = false; };
  }, [navigate, sourceExchangeId, sourceResponseEvent]);

  async function selectRule(rule: RuleDefinition_Serialize) {
    setLoadingEditor(true);
    setFieldErrors({});
    try {
      const [definition, editorContext] = await Promise.all([
        callCommand(commands.ruleDefinitionGet(rule.rule_id)),
        callCommand(commands.ruleEditorContext(rule.listener_id)),
      ]);
      setSelected(definition);
      setInput(toSaveInput(definition));
      setContext(editorContext);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setLoadingEditor(false);
    }
  }

  function startCreation(draft: RuleDefinitionSaveInput, editorContext: RuleEditorContext) {
    setSelected(undefined);
    setInput(draft);
    setContext(editorContext);
    setFieldErrors({});
    setCreationOpen(false);
  }

  async function save() {
    if (!input || pending) return;
    setPending(true);
    try {
      const saved = await callCommand(commands.ruleDefinitionSave(input));
      setSelected(saved);
      setInput(toSaveInput(saved));
      setFieldErrors({});
      await source.refresh();
      toast(`规则“${saved.name}”已保存。`, { variant: "success" });
    } catch (reason) {
      setFieldErrors(appErrorViewModel(reason)?.field_errors ?? {});
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(false);
    }
  }

  async function toggle(enabled: boolean) {
    if (!selected || !input || pending) return;
    setPending(true);
    try {
      const saved = await callCommand(commands.ruleDefinitionToggle(selected.rule_id, selected.revision, enabled));
      setSelected(saved);
      setInput(toSaveInput(saved));
      await source.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(false);
    }
  }

  async function copy() {
    if (!selected || pending) return;
    setPending(true);
    try {
      const copied = await callCommand(commands.ruleDefinitionCopy(selected.rule_id));
      const editorContext = await callCommand(commands.ruleEditorContext(copied.listener_id));
      setSelected(copied);
      setInput(toSaveInput(copied));
      setContext(editorContext);
      await source.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(false);
    }
  }

  async function remove() {
    if (!selected || pending) return;
    setPending(true);
    try {
      const result = await callCommand(commands.ruleDefinitionDelete(selected.rule_id, selected.revision, true));
      if (!result.success || result.entity_id !== selected.rule_id) throw new Error("规则删除响应无效。");
      setSelected(undefined);
      setInput(undefined);
      setContext(undefined);
      await source.refresh();
      toast(result.message, { variant: "success" });
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(false);
    }
  }

  return (
    <div aria-label="统一规则工作区" className="h-full min-h-0 overflow-auto p-3">
      <div className="h-full min-h-[42rem] max-[1280px]:h-auto">
        <RulesWorkspaceShell>
          <RuleDefinitionList
            error={source.error}
            loading={source.isLoading}
            onNew={() => setCreationOpen(true)}
            onRefresh={() => void source.refresh()}
            onSelect={(rule) => void selectRule(rule)}
            pending={pending || loadingEditor}
            rules={source.rules}
            selectedId={selected?.rule_id}
          />
          <RuleDefinitionEditor
            context={context}
            fieldErrors={fieldErrors}
            input={input}
            listener={listener}
            loading={loadingEditor}
            onChange={(change) => {
              setInput((current) => typeof change === "function" ? (current ? change(current) : current) : change);
              setFieldErrors({});
            }}
            onCopy={() => void copy()}
            onDelete={() => void remove()}
            onSave={() => void save()}
            onToggle={(enabled) => void toggle(enabled)}
            pending={pending}
          />
        </RulesWorkspaceShell>
      </div>
      <RuleCreationDialog listeners={source.listeners} onClose={() => setCreationOpen(false)} onCreate={startCreation} open={creationOpen} />
    </div>
  );
}

function toSaveInput(rule: RuleDefinition_Serialize): RuleDefinitionSaveInput {
  return {
    rule_id: rule.rule_id,
    expected_revision: rule.revision,
    draft: {
      name: rule.name,
      enabled: rule.enabled,
      priority: rule.priority,
      listener_id: rule.listener_id,
      stage: rule.stage,
      one_shot: rule.one_shot,
      content: rule.content,
    },
  };
}
