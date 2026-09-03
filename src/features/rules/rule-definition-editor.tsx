import { Button, Input, Label, TextArea, TextField } from "@heroui/react";
import type { HttpRuleEditorStageViewModel, ProxyListener, RuleDefinitionSaveInput, RuleEditorContext, SocketRuleEditorStageViewModel } from "@/generated/rust-types";
import { ruleStageIncompatibility } from "./rule-definition-model";
import { RuleMetadataFields } from "./rule-metadata-fields";
import { RuleSinglePairEditor } from "./rule-single-pair-editor";

type Stage = HttpRuleEditorStageViewModel | SocketRuleEditorStageViewModel;
type RuleDefinitionChange = RuleDefinitionSaveInput | ((current: RuleDefinitionSaveInput) => RuleDefinitionSaveInput);

export function RuleDefinitionEditor(props: {
  input?: RuleDefinitionSaveInput;
  context?: RuleEditorContext;
  listener?: ProxyListener;
  loading: boolean;
  pending: boolean;
  fieldErrors: Record<string, string[]>;
  onChange: (change: RuleDefinitionChange) => void;
  onSave: (input: RuleDefinitionSaveInput) => void;
  onCopy: () => void;
  onDelete: () => void;
}) {
  if (props.loading) return <EditorShell><p>正在读取规则…</p></EditorShell>;
  if (!props.input || !props.listener) return <EditorShell><p className="text-sm text-[var(--telemetry-muted)]">选择一条规则或新建规则进行编辑。</p></EditorShell>;

  const input = props.input;
  const existing = input.rule_id != null;
  const stage = editorStage(props.context, input.draft.stage);
  const stageOptions = (props.context?.content.value.stages ?? []).map((item) => ({ item, reason: ruleStageIncompatibility(input, props.context, item.stage) }));
  const currentStageReason = ruleStageIncompatibility(input, props.context, input.draft.stage);
  const updateDraft = (draft: RuleDefinitionSaveInput["draft"]) => props.onChange({ ...input, draft });
  const updateDescription = (description: string) => {
    if (input.draft.content.type !== "http") return;
    updateDraft({ ...input.draft, content: { type: "http", value: { ...input.draft.content.value, description } } });
  };

  return <EditorShell>
    <>
      <header><h2 className="text-lg font-semibold">{existing ? "编辑规则" : "新建规则"}</h2><p className="text-xs text-[var(--telemetry-muted)]">Listener 创建后不可切换。</p></header>
      <RuleMetadataFields
        enabled={input.draft.enabled}
        listenerControl={<TextField isDisabled><Label>Listener</Label><Input aria-label="固定 Listener" value={props.listener.name} /></TextField>}
        name={input.draft.name}
        pending={props.pending}
        priority={input.draft.priority}
        stage={input.draft.stage}
        stageOptions={stageOptions.map(({ item, reason }) => ({ stage: item.stage, reason }))}
        onEnabledChange={(enabled) => updateDraft({ ...input.draft, enabled })}
        onNameChange={(name) => updateDraft({ ...input.draft, name })}
        onPriorityChange={(priority) => updateDraft({ ...input.draft, priority })}
        onStageChange={(stage) => { if (!ruleStageIncompatibility(input, props.context, stage)) updateDraft({ ...input.draft, stage }); }}
      />
    </>
    {currentStageReason && <p className="text-sm text-red-600" role="alert">当前阶段不可保存：{currentStageReason}</p>}
    {input.draft.content.type === "http" ? <section className="space-y-4"><h3 className="font-semibold">HTTP 规则内容</h3><TextField><Label>说明</Label><TextArea className="min-h-20" value={input.draft.content.value.description} onChange={(event) => updateDescription(event.target.value)} /></TextField></section> : <h3 className="font-semibold">Socket Document 规则内容</h3>}
    <RuleSinglePairEditor actions={existing ? <><Button isDisabled={props.pending} variant="outline" onPress={props.onCopy}>复制规则</Button><Button isDisabled={props.pending} variant="danger-soft" onPress={props.onDelete}>删除规则</Button></> : undefined} conditionPath={props.context?.document_condition_path} input={input} key={editorKey(input)} localTypes={props.context?.local_document_types ?? []} onSave={props.onSave} pending={props.pending || input.draft.name.trim() === "" || currentStageReason != null} stage={stage} />
    {Object.values(props.fieldErrors).flat().length > 0 && <p className="text-sm text-red-600" role="alert">{Object.values(props.fieldErrors).flat().join("；")}</p>}
  </EditorShell>;
}

function EditorShell({ children }: { children: React.ReactNode }) { return <div className="space-y-5 p-1">{children}</div>; }
function editorStage(context: RuleEditorContext | undefined, stage: RuleDefinitionSaveInput["draft"]["stage"]): Stage | undefined { return context?.content.value.stages.find((item) => item.stage === stage); }
function editorKey(input: RuleDefinitionSaveInput) { return `${input.rule_id ?? "new"}:${input.draft.listener_id}:${input.draft.stage}`; }
