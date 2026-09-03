import { useState } from "react";
import { Button, Label, ListBox, Select, TextArea, TextField } from "@heroui/react";
import type { ProxyListener, RuleDefinitionSaveInput, RuleEditorContext, RuleNewDefinitionDraft } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { RuleMetadataFields } from "./rule-metadata-fields";
import { RuleSinglePairEditor } from "./rule-single-pair-editor";

export function RuleCreationEditor(props: {
  listeners: ProxyListener[];
  onCancel: () => void;
  onSave: (draft: RuleDefinitionSaveInput) => void;
  pending: boolean;
  fieldErrors: Record<string, string[]>;
}) {
  const [context, setContext] = useState<RuleEditorContext>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const [structure, setStructure] = useState<RuleNewDefinitionDraft>();
  const [name, setName] = useState("");
  const [enabled, setEnabled] = useState(false);
  const [priority, setPriority] = useState<number>();
  const [description, setDescription] = useState("");
  const stages = context?.content.value.stages ?? [];

  async function selectListener(listenerId: string) {
    setContext(undefined);
    setStructure(undefined);
    setEnabled(false);
    setPriority(undefined);
    setDescription("");
    setError(undefined);
    if (!listenerId) return;
    setLoading(true);
    try {
      setContext(await callCommand(commands.ruleEditorContext(listenerId)));
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }

  const ready = context != null && structure != null && name.trim() !== ""
    && priority != null && Number.isSafeInteger(priority) && priority >= 0;
  const stage = context?.content.value.stages.find((item) => item.stage === structure?.stage);

  return (
    <section aria-label="创建统一规则" className="space-y-4 p-1">
      <header className="flex items-center"><h2 className="text-lg font-semibold">新建规则</h2><Button className="ml-auto" variant="ghost" onPress={props.onCancel}>取消</Button></header>
      <RuleMetadataFields
        enabled={enabled}
        listenerControl={<Select aria-label="创建规则的 Listener" isDisabled={loading || props.listeners.length === 0} selectedKey={context?.listener_id ?? null} onSelectionChange={(key) => void selectListener(String(key))}>
          <Label>Listener</Label><Select.Trigger><Select.Value>{({ selectedText }) => selectedText || "选择 Listener"}</Select.Value><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>{props.listeners.map((listener) => <ListBox.Item id={listener.id} key={listener.id} textValue={`${listener.name} · ${listener.data_plane.kind === "http" ? "HTTP" : "Socket"}`}>{listener.name} · {listener.data_plane.kind === "http" ? "HTTP" : "Socket"}</ListBox.Item>)}</ListBox></Select.Popover>
        </Select>}
        name={name}
        pending={loading}
        priority={priority}
        stage={structure?.stage}
        stageOptions={stages.map(({ stage }) => ({ stage }))}
        onEnabledChange={setEnabled}
        onNameChange={setName}
        onPriorityChange={setPriority}
        onStageChange={(next) => setStructure(stages.find(({ stage }) => stage === next)?.new_rule_draft)}
      />
      {props.listeners.length === 0 && <p className="text-sm">当前 Workspace 没有可用于创建规则的 Listener。</p>}
      {loading && <p>正在读取 Rust 规则能力…</p>}
      {error && <p role="alert" className="text-sm text-red-600">{error}</p>}
      {structure?.content.type === "http" && <section className="space-y-4"><h3 className="font-semibold">HTTP 规则内容</h3><TextField><Label>说明</Label><TextArea className="min-h-20" value={description} onChange={(event) => setDescription(event.target.value)} /></TextField></section>}
      {structure?.content.type === "socket" && <h3 className="font-semibold">Socket Document 规则内容</h3>}
      {context && structure && priority != null && <RuleSinglePairEditor
        conditionPath={context.document_condition_path}
        creation={{ structure, name, enabled, priority, description }}
        key={`${structure.listener_id}:${structure.stage}`}
        localTypes={context.local_document_types}
        onSave={props.onSave}
        pending={props.pending || !ready}
        stage={stage}
      />}
      {Object.values(props.fieldErrors).flat().length > 0 && <p className="text-sm text-red-600" role="alert">{Object.values(props.fieldErrors).flat().join("；")}</p>}
    </section>
  );
}
