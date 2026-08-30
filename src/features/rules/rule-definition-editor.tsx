import { useState } from "react";
import { Button, Input, Label, ListBox, NumberField, Select, Switch, TextArea, TextField } from "@heroui/react";
import type {
  Condition,
  ConditionTree,
  DocumentMutation,
  HttpRuleEditorStage,
  ProtocolRuleCommonActionCapability,
  ProtocolRuleFieldCapability,
  ProxyListener,
  RuleAction,
  RuleActionKind,
  RuleConditionKind,
  RuleDefinitionSaveInput,
  RuleEditorContext,
  SocketRuleEditorStage,
  UnifiedAction,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { ruleStageIncompatibility, ruleStageLabel } from "./rule-definition-model";
import { useAsyncRequestSlots } from "./use-async-request-slots";

type EditorStage = HttpRuleEditorStage | SocketRuleEditorStage;
type RuleDefinitionChange = RuleDefinitionSaveInput | ((current: RuleDefinitionSaveInput) => RuleDefinitionSaveInput);

export function RuleDefinitionEditor(props: {
  input?: RuleDefinitionSaveInput;
  context?: RuleEditorContext;
  listener?: ProxyListener;
  loading: boolean;
  pending: boolean;
  fieldErrors: Record<string, string[]>;
  onChange: (change: RuleDefinitionChange) => void;
  onSave: () => void;
  onCopy: () => void;
  onToggle: (enabled: boolean) => void;
  onDelete: () => void;
}) {
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  if (props.loading) return <EditorShell><p>正在读取规则…</p></EditorShell>;
  if (!props.input || !props.listener) {
    return <EditorShell><p className="text-sm text-[var(--telemetry-muted)]">选择一条规则或新建规则进行编辑。</p></EditorShell>;
  }
  const { input } = props;
  const existing = input.rule_id != null;
  const stage = editorStage(props.context, input.draft.stage);
  const stageOptions = (props.context?.content.value.stages ?? []).map((item) => ({
    item,
    reason: ruleStageIncompatibility(input, props.context, item.stage),
  }));
  const currentStageReason = ruleStageIncompatibility(input, props.context, input.draft.stage);
  const updateDraft = (draft: RuleDefinitionSaveInput["draft"]) => props.onChange({ ...input, draft });

  return (
    <EditorShell>
      <header><h2 className="text-lg font-semibold">{existing ? "编辑规则" : "新建规则"}</h2><p className="text-xs text-[var(--telemetry-muted)]">Listener 创建后不可切换。</p></header>
      <TextField isDisabled={props.pending}><Label>规则名称</Label><Input aria-label="规则名称" maxLength={128} value={input.draft.name} onChange={(event) => updateDraft({ ...input.draft, name: event.target.value })} /></TextField>
      <div className="grid gap-3 sm:grid-cols-2">
        <TextField isDisabled><Label>Listener</Label><Input aria-label="固定 Listener" value={props.listener.name} /></TextField>
        <Select aria-label="处理阶段" isDisabled={props.pending || !props.context} selectedKey={input.draft.stage} onSelectionChange={(key) => {
          const nextStage = String(key) as typeof input.draft.stage;
          if (!ruleStageIncompatibility(input, props.context, nextStage)) updateDraft({ ...input.draft, stage: nextStage });
        }}>
          <Label>处理阶段</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>{stageOptions.map(({ item, reason }) => <ListBox.Item id={item.stage} isDisabled={reason != null} key={item.stage} textValue={`${ruleStageLabel(item.stage)}${reason ? ` ${reason}` : ""}`}>
            <span className="block">{ruleStageLabel(item.stage)}</span>
            {reason && <span className="block text-xs text-red-600">{reason}</span>}
          </ListBox.Item>)}</ListBox></Select.Popover>
        </Select>
      </div>
      <div className="grid items-center gap-3 sm:grid-cols-2">
        <Switch aria-label="启用规则" isDisabled={props.pending} isSelected={input.draft.enabled} onChange={(enabled) => existing ? props.onToggle(enabled) : updateDraft({ ...input.draft, enabled })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control>启用规则</Switch.Content></Switch>
        <NumberField aria-label="阶段内优先级" isDisabled={props.pending} value={input.draft.priority} onChange={(priority) => updateDraft({ ...input.draft, priority })}><Label>阶段内优先级</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
      </div>
      <p className="text-xs text-[var(--telemetry-muted)]">{ruleStageLabel(input.draft.stage)} · priority 只与此阶段及同一执行作用域中的规则比较。</p>
      {currentStageReason && <p role="alert" className="text-sm text-red-600">当前阶段不可保存：{currentStageReason}</p>}
      {input.draft.content.type === "http" ? (
        <HttpContentEditor input={input} onChange={props.onChange} stage={stage && "http" in stage ? stage : undefined} />
      ) : (
        <SocketContentEditor input={input} onChange={props.onChange} stage={stage && "fields" in stage ? stage : undefined} />
      )}
      {Object.values(props.fieldErrors).flat().length > 0 && <p role="alert" className="text-sm text-red-600">{Object.values(props.fieldErrors).flat().join("；")}</p>}
      <div className="flex gap-2">
        <Button isDisabled={props.pending || input.draft.name.trim() === "" || currentStageReason != null} variant="primary" onPress={props.onSave}>保存规则</Button>
        {existing && <Button isDisabled={props.pending} variant="outline" onPress={props.onCopy}>复制规则</Button>}
        {existing && <Button isDisabled={props.pending} variant="danger-soft" onPress={() => setConfirmingDelete(true)}>删除规则</Button>}
      </div>
      {confirmingDelete && <div className="rounded-lg border border-red-500 p-3" role="alertdialog"><p>删除后无法恢复。</p><div className="mt-2 flex gap-2"><Button variant="danger" onPress={props.onDelete}>确认删除</Button><Button variant="outline" onPress={() => setConfirmingDelete(false)}>取消</Button></div></div>}
    </EditorShell>
  );
}

function HttpContentEditor(props: { input: RuleDefinitionSaveInput; stage?: HttpRuleEditorStage; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "http") return null;
  const value = props.input.draft.content.value;
  const scope = editorScope(props.input);
  const update = (next: typeof value) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "http", value: next } } });
  return <section className="space-y-4"><h3 className="font-semibold">HTTP 规则内容</h3>
    <TextField><Label>说明</Label><TextArea className="min-h-20" value={value.description} onChange={(event) => update({ ...value, description: event.target.value })} /></TextField>
    <section className="space-y-2 rounded-lg border border-[var(--telemetry-line)] p-3"><h4 className="font-medium">HTTP Header、URL 与请求信息</h4>
      <p className="text-xs text-[var(--telemetry-muted)]">条件 {conditionLeafCount(value.condition)} 个 · 动作 {value.actions.length} 个</p>
      <CapabilityList labels={[
        ...(props.stage?.http?.match_field_kinds ?? []).map(matchCapabilityLabel),
        ...(props.stage?.http?.actions ?? []).map((action) => httpActionLabel(action.kind)),
      ]} />
      {props.stage?.http && <HttpFactoryControls
        actionKinds={props.stage.http.actions.map((action) => action.kind)}
        actions={value.actions}
        condition={value.condition}
        editorScope={scope}
        key={scope}
        onChange={(condition, actions) => update({ ...value, condition, actions })}
        onCreateAction={(action) => props.onChange((current) => appendHttpFactoryResult(current, scope, "action", action))}
        onCreateCondition={(condition) => props.onChange((current) => appendHttpFactoryResult(current, scope, "condition", condition))}
        stage={props.stage.http.stage}
      />}
    </section>
    <section className="space-y-2 rounded-lg border border-[var(--telemetry-line)] p-3"><h4 className="font-medium">HTTP Body Document</h4>
      {value.document ? <>
        <Button size="sm" variant="outline" onPress={() => update({ ...value, document: null })}>移除 HTTP Body Document</Button>
        <DocumentEditor
          actions={value.actions}
          commonActions={props.stage?.document_common_actions ?? []}
          condition={value.condition}
          editorScope={scope}
          fields={props.stage?.document_fields ?? []}
          key={`${scope}:document`}
          packageLabel={`${value.document.package.id}@${value.document.package.version}`}
          onActionsChange={(actions) => update({ ...value, actions })}
          onConditionChange={(condition) => update({ ...value, condition })}
          onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, scope, "action", action))}
          onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, scope, "condition", condition))}
        />
      </> : <OptionalHttpDocument stage={props.stage} onAdd={(document) => update({ ...value, document })} />}
    </section>
  </section>;
}

function HttpFactoryControls(props: {
  actionKinds: RuleActionKind[];
  actions: UnifiedAction[];
  condition: ConditionTree;
  editorScope: string;
  stage: import("@/generated/rust-types").MessageStage;
  onChange: (condition: ConditionTree, actions: UnifiedAction[]) => void;
  onCreateAction: (action: RuleAction) => void;
  onCreateCondition: (condition: import("@/generated/rust-types").MatchCondition) => void;
}) {
  const [error, setError] = useState<string>();
  const { pending, runAsync } = useAsyncRequestSlots(props.editorScope);
  function addCondition(kind: RuleConditionKind) {
    setError(undefined);
    return runAsync(
      "condition",
      () => callCommand(commands.ruleDefinitionConditionDraft(kind, props.stage)),
      props.onCreateCondition,
      (reason) => setError(errorMessage(reason)),
    );
  }
  function addAction(kind: RuleActionKind) {
    setError(undefined);
    return runAsync(
      "action",
      () => callCommand(commands.ruleDefinitionActionDraft(kind, props.stage)),
      props.onCreateAction,
      (reason) => setError(errorMessage(reason)),
    );
  }
  return <div className="space-y-2">
    <div className="flex flex-wrap gap-1">
      <Button isDisabled={pending} size="sm" variant="outline" onPress={() => void addCondition("field")}>添加条件：字段</Button>
      <Button isDisabled={pending} size="sm" variant="outline" onPress={() => void addCondition("nth_hit")}>添加条件：第 N 次命中</Button>
      {props.actionKinds.map((kind) => <Button isDisabled={pending} key={kind} size="sm" variant="outline" onPress={() => void addAction(kind)}>添加动作：{httpActionLabel(kind)}</Button>)}
    </div>
    {conditionLeaves(props.condition).filter((leaf) => leaf.source === "http").map((leaf, index) => <div className="flex items-center rounded-md border p-2 text-xs" key={index}><span>{httpConditionLabel(leaf.condition)}</span></div>)}
    {props.actions.map((action, index) => <div className="flex items-center rounded-md border p-2 text-xs" key={index}><span>{unifiedActionLabel(action)}</span><Button className="ml-auto" size="sm" variant="ghost" onPress={() => props.onChange(props.condition, props.actions.filter((_, itemIndex) => itemIndex !== index))}>删除</Button></div>)}
    {error && <p role="alert" className="text-xs text-red-600">{error}</p>}
  </div>;
}

function SocketContentEditor(props: { input: RuleDefinitionSaveInput; stage?: SocketRuleEditorStage; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "socket") return null;
  const value = props.input.draft.content.value;
  return <section className="space-y-3"><h3 className="font-semibold">Socket Document 规则内容</h3>
    <DocumentEditor actions={value.actions} commonActions={props.stage?.common_actions ?? []} condition={value.condition} editorScope={editorScope(props.input)} fields={props.stage?.fields ?? []} key={`${editorScope(props.input)}:document`} packageLabel={`${value.package.id}@${value.package.version}`} onActionsChange={(actions) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, actions } } } })} onConditionChange={(condition) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, condition } } } })} onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "action", action))} onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "condition", condition))} />
  </section>;
}

function OptionalHttpDocument(props: { stage?: HttpRuleEditorStage; onAdd: (document: NonNullable<import("@/generated/rust-types").HttpRuleContent["document"]>) => void }) {
  const content = props.stage?.new_rule_draft.draft.content;
  const document = content?.type === "http" ? content.value.document : null;
  if (!document) return <p className="text-sm text-[var(--telemetry-muted)]">当前 Listener/阶段没有协议 Body Document 能力。</p>;
  return <div className="space-y-2"><p className="text-sm text-[var(--telemetry-muted)]">当前规则仅处理 HTTP Header；可按 Rust 草稿启用 Body Document。</p><Button size="sm" variant="outline" onPress={() => props.onAdd(document)}>添加 HTTP Body Document</Button></div>;
}

function DocumentEditor(props: { packageLabel: string; editorScope: string; fields: ProtocolRuleFieldCapability[]; commonActions: ProtocolRuleCommonActionCapability[]; condition: ConditionTree; actions: UnifiedAction[]; onCreateCondition: (condition: Condition) => void; onCreateAction: (action: UnifiedAction) => void; onConditionChange: (condition: ConditionTree) => void; onActionsChange: (actions: UnifiedAction[]) => void }) {
  const [rawValues, setRawValues] = useState<Record<string, string>>({});
  const [error, setError] = useState<string>();
  const { pending, runAsync } = useAsyncRequestSlots(`${props.editorScope}:document`);
  const createWithValue = (field: ProtocolRuleFieldCapability, kind: "condition" | "set_field") => {
    const raw = rawValues[field.name];
    if (raw == null) return;
    setError(undefined);
    void runAsync(`${kind}:${field.name}`, () => callCommand(commands.ruleParseDocumentValue(field.type, raw)), (value) => {
      if (kind === "condition") props.onCreateCondition({ source: "document", path: field.name, predicate: predicateFor(field.type, value) });
      else props.onCreateAction({ source: "document", value: { type: "set", path: field.name, value } });
    }, (reason) => setError(errorMessage(reason)));
  };
  const documentConditions = conditionLeaves(props.condition).filter((condition) => condition.source === "document");
  const documentActions = props.actions.filter((action) => action.source === "document" || action.source === "record_match");
  return <div className="space-y-2"><p className="text-sm font-medium">{props.packageLabel}</p><p className="text-xs text-[var(--telemetry-muted)]">字段 {props.fields.length} 个 · 条件 {documentConditions.length} 个 · Document 动作 {documentActions.length} 个</p>
    {props.fields.map((field) => <div className="space-y-2 rounded-md border p-2" key={field.name}><TextField><Label>Document 值：{field.label}</Label><Input aria-label={`Document 值：${field.label}`} value={rawValues[field.name] ?? ""} onChange={(event) => setRawValues((current) => ({ ...current, [field.name]: event.target.value }))} /></TextField><div className="flex flex-wrap gap-2">
      {field.operators.includes("equals") && <Button isDisabled={pending || rawValues[field.name] == null} size="sm" variant="outline" onPress={() => createWithValue(field, "condition")}>添加条件：{field.label} equals</Button>}
      {field.actions.includes("set_field") && <Button isDisabled={pending || rawValues[field.name] == null} size="sm" variant="outline" onPress={() => createWithValue(field, "set_field")}>添加动作：Set {field.label}</Button>}
      {field.actions.includes("clear_field") && <Button isDisabled={pending} size="sm" variant="outline" onPress={() => props.onCreateAction({ source: "document", value: { type: "clear", path: field.name } })}>添加动作：Clear {field.label}</Button>}
    </div></div>)}
    <div className="flex flex-wrap gap-2">
      {props.commonActions.includes("record_match") && <Button size="sm" variant="outline" onPress={() => props.onCreateAction({ source: "record_match" })}>添加：记录命中</Button>}
    </div>
    {documentConditions.map((condition, index) => <div className="flex items-center rounded-md border p-2 text-sm" key={`${condition.path}-${index}`}><span>{condition.path}</span></div>)}
    {documentActions.map((action, index) => <div className="flex items-center rounded-md border p-2 text-sm" key={`${action.source}-${index}`}><span>{unifiedActionLabel(action)}</span></div>)}
    {error && <p role="alert" className="text-xs text-red-600">{error}</p>}
  </div>;
}

function EditorShell({ children }: { children: React.ReactNode }) {
  return <aside className="space-y-5 overflow-auto p-5">{children}</aside>;
}

function editorStage(context: RuleEditorContext | undefined, stage: RuleDefinitionSaveInput["draft"]["stage"]): EditorStage | undefined {
  return context?.content.value.stages.find((item) => item.stage === stage);
}

function editorScope(input: RuleDefinitionSaveInput) {
  return `${input.rule_id ?? "new"}:${input.draft.listener_id}:${input.draft.stage}`;
}

function appendHttpFactoryResult(
  input: RuleDefinitionSaveInput,
  expectedScope: string,
  kind: "condition" | "action",
  value: import("@/generated/rust-types").MatchCondition | RuleAction,
) {
  if (editorScope(input) !== expectedScope || input.draft.content.type !== "http") return input;
  const content = input.draft.content.value;
  const next = kind === "condition"
    ? { ...content, condition: appendCondition(content.condition, { source: "http", condition: value as import("@/generated/rust-types").MatchCondition }) }
    : { ...content, actions: [...content.actions, wrapRuleAction(value as RuleAction)] };
  return { ...input, draft: { ...input.draft, content: { type: "http" as const, value: next } } };
}

function appendDocumentResult(input: RuleDefinitionSaveInput, expectedScope: string, kind: "condition" | "action", value: Condition | UnifiedAction) {
  if (editorScope(input) !== expectedScope) return input;
  const content = input.draft.content;
  if (content.type === "http") {
    if (!content.value.document) return input;
    const next = kind === "condition"
      ? { ...content.value, condition: appendCondition(content.value.condition, value as Condition) }
      : { ...content.value, actions: [...content.value.actions, value as UnifiedAction] };
    return { ...input, draft: { ...input.draft, content: { type: "http" as const, value: next } } };
  }
  const next = kind === "condition"
    ? { ...content.value, condition: appendCondition(content.value.condition, value as Condition) }
    : { ...content.value, actions: [...content.value.actions, value as UnifiedAction] };
  return { ...input, draft: { ...input.draft, content: { type: "socket" as const, value: next } } };
}

function CapabilityList({ labels }: { labels: string[] }) {
  return labels.length > 0 ? <div className="flex flex-wrap gap-1">{labels.map((label) => <span className="rounded-full bg-[var(--telemetry-soft)] px-2 py-1 text-xs" key={label}>{label}</span>)}</div> : <p className="text-xs text-[var(--telemetry-muted)]">Rust 未声明此阶段的 HTTP 能力。</p>;
}

function matchCapabilityLabel(kind: string) {
  return ({ terminal_ip: "终端 IP", certificate_fingerprint: "证书指纹", path_or_request_type: "URL / 请求类型", json_path: "JSON Path" } as Record<string, string>)[kind] ?? kind;
}

function httpActionLabel(kind: string) {
  return ({ set_header: "Set Header", set_json_field: "Set JSON Field", replace_body_text: "Replace Body", mock_response: "Mock Response", delay: "Delay" } as Record<string, string>)[kind] ?? kind;
}

function httpRuleActionLabel(action: RuleAction) {
  if (typeof action === "string") return action === "Pause" ? "Pause" : action;
  if ("SetHeader" in action) return "Set Header";
  if ("SetJsonField" in action) return "Set JSON Field";
  if ("ReplaceBodyText" in action) return "Replace Body";
  if ("Delay" in action) return "Delay";
  if ("Jitter" in action) return "Jitter";
  if ("Throttle" in action) return "Throttle";
  if ("Intermittent" in action) return "Intermittent";
  if ("CustomHttpStatus" in action) return "Custom HTTP Status";
  if ("Terminal" in action && typeof action.Terminal === "object" && "MockResponse" in action.Terminal) return "Mock Response";
  return "Terminal";
}

function httpConditionLabel(condition: import("@/generated/rust-types").MatchCondition) {
  return "NthHit" in condition ? `第 ${condition.NthHit} 次命中` : "字段条件";
}

function appendCondition(tree: ConditionTree, condition: Condition): ConditionTree {
  const leaf: ConditionTree = { operator: "leaf", children: condition };
  return tree.operator === "all"
    ? { ...tree, children: [...tree.children, leaf] }
    : { operator: "all", children: [tree, leaf] };
}

function conditionLeaves(tree: ConditionTree): Condition[] {
  return tree.operator === "leaf" ? [tree.children] : tree.children.flatMap(conditionLeaves);
}

function conditionLeafCount(tree: ConditionTree) {
  return conditionLeaves(tree).length;
}

function wrapRuleAction(action: RuleAction): UnifiedAction {
  return typeof action === "object" && "Terminal" in action
    ? { source: "terminal", value: action.Terminal! }
    : { source: "http", value: action };
}

function predicateFor(type: ProtocolRuleFieldCapability["type"], value: import("@/generated/rust-types").DocumentValue): import("@/generated/rust-types").DocumentPredicate {
  if (type === "string" && typeof value === "string") return { type: "string", value: { operator: "equal", value } };
  if (type === "number" && typeof value === "number") return { type: "number", value: { operator: "equal", value } };
  if (type === "boolean" && typeof value === "boolean") return { type: "boolean", value: { equal: value } };
  throw new Error(`Rust returned a ${type} capability with a mismatched parsed value`);
}

function unifiedActionLabel(action: UnifiedAction) {
  if (action.source === "record_match") return "记录命中";
  if (action.source === "http") return httpRuleActionLabel(action.value);
  if (action.source === "terminal") return httpRuleActionLabel({ Terminal: action.value });
  const mutation: DocumentMutation = action.value;
  if (mutation.type === "clear") return `清除字段 ${mutation.path}`;
  if (mutation.type === "insert") return `插入 ${mutation.path}[${mutation.index}]`;
  if (mutation.type === "append") return `追加到 ${mutation.path}`;
  return `设置字段 ${mutation.path}`;
}
