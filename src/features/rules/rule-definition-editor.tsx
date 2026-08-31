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
  HttpAction,
  RuleActionKind,
  RuleConditionKind,
  RuleDefinitionSaveInput,
  RuleEditorContext,
  RuleLocalDocumentActionKind,
  RuleLocalDocumentPredicateKind,
  RuleLocalDocumentTypeCapability,
  RuleLocalDocumentValueType,
  SocketRuleEditorStage,
  UnifiedAction,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { ruleStageIncompatibility, ruleStageLabel } from "./rule-definition-model";
import { documentEditorFields, ruleLocalFields, type DocumentEditorField } from "./rule-document-fields";
import { ConditionTreeEditor, DocumentMetadataTree, OrderedActionList } from "./rule-tree-editors";
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
        <HttpContentEditor input={input} localTypes={props.context?.local_document_types ?? []} onChange={props.onChange} stage={stage && "http" in stage ? stage : undefined} />
      ) : (
        <SocketContentEditor input={input} localTypes={props.context?.local_document_types ?? []} onChange={props.onChange} stage={stage && "fields" in stage ? stage : undefined} />
      )}
      <ConditionTreeEditor tree={ruleCondition(input)} onChange={(condition) => props.onChange(updateRuleCondition(input, condition))} />
      <OrderedActionList actions={ruleActions(input)} label={unifiedActionLabel} onChange={(actions) => props.onChange(updateRuleActions(input, actions))} />
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

function HttpContentEditor(props: { input: RuleDefinitionSaveInput; stage?: HttpRuleEditorStage; localTypes: RuleLocalDocumentTypeCapability[]; onChange: (change: RuleDefinitionChange) => void }) {
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
          localTypes={props.localTypes}
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
  onCreateAction: (action: HttpAction) => void;
  onCreateCondition: (condition: Condition) => void;
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
    {error && <p role="alert" className="text-xs text-red-600">{error}</p>}
  </div>;
}

function SocketContentEditor(props: { input: RuleDefinitionSaveInput; stage?: SocketRuleEditorStage; localTypes: RuleLocalDocumentTypeCapability[]; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "socket") return null;
  const value = props.input.draft.content.value;
  return <section className="space-y-3"><h3 className="font-semibold">Socket Document 规则内容</h3>
    <DocumentEditor actions={value.actions} commonActions={props.stage?.common_actions ?? []} condition={value.condition} editorScope={editorScope(props.input)} fields={props.stage?.fields ?? []} localTypes={props.localTypes} key={`${editorScope(props.input)}:document`} packageLabel={`${value.package.id}@${value.package.version}`} onActionsChange={(actions) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, actions } } } })} onConditionChange={(condition) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, condition } } } })} onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "action", action))} onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "condition", condition))} />
  </section>;
}

function OptionalHttpDocument(props: { stage?: HttpRuleEditorStage; onAdd: (document: NonNullable<import("@/generated/rust-types").HttpRuleContent["document"]>) => void }) {
  const content = props.stage?.new_rule_draft.draft.content;
  const document = content?.type === "http" ? content.value.document : null;
  if (!document) return <p className="text-sm text-[var(--telemetry-muted)]">当前 Listener/阶段没有协议 Body Document 能力。</p>;
  return <div className="space-y-2"><p className="text-sm text-[var(--telemetry-muted)]">当前规则仅处理 HTTP Header；可按 Rust 草稿启用 Body Document。</p><Button size="sm" variant="outline" onPress={() => props.onAdd(document)}>添加 HTTP Body Document</Button></div>;
}

function DocumentEditor(props: { packageLabel: string; editorScope: string; fields: ProtocolRuleFieldCapability[]; localTypes: RuleLocalDocumentTypeCapability[]; commonActions: ProtocolRuleCommonActionCapability[]; condition: ConditionTree; actions: UnifiedAction[]; onCreateCondition: (condition: Condition) => void; onCreateAction: (action: UnifiedAction) => void; onConditionChange: (condition: ConditionTree) => void; onActionsChange: (actions: UnifiedAction[]) => void }) {
  const [rawValues, setRawValues] = useState<Record<string, string>>({});
  const [fieldIndices, setFieldIndices] = useState<Record<string, number>>({});
  const [localPath, setLocalPath] = useState("");
  const [localType, setLocalType] = useState<RuleLocalDocumentValueType | "">("");
  const [localPredicate, setLocalPredicate] = useState<RuleLocalDocumentPredicateKind | "">("");
  const [localAction, setLocalAction] = useState<RuleLocalDocumentActionKind | "">("");
  const [localIndex, setLocalIndex] = useState<number>(0);
  const [localValue, setLocalValue] = useState("");
  const [error, setError] = useState<string>();
  const { pending, runAsync } = useAsyncRequestSlots(`${props.editorScope}:document`);
  const requestCondition = (path: string, type: RuleLocalDocumentValueType, predicate: RuleLocalDocumentPredicateKind, raw: string, key: string) => {
    setError(undefined);
    void runAsync(key, () => callCommand(commands.ruleDefinitionDocumentConditionDraft(path, type, predicate, raw)), props.onCreateCondition, (reason) => setError(errorMessage(reason)));
  };
  const requestAction = (path: string, type: RuleLocalDocumentValueType, action: RuleLocalDocumentActionKind, raw: string | null, index: number | null, key: string) => {
    setError(undefined);
    void runAsync(key, () => callCommand(commands.ruleDefinitionDocumentActionDraft(path, type, action, raw, index)), props.onCreateAction, (reason) => setError(errorMessage(reason)));
  };
  const createPredicate = (field: DocumentEditorField, predicate: RuleLocalDocumentPredicateKind) => {
    const raw = rawValues[field.name];
    if (raw == null) return;
    requestCondition(field.name, field.type, predicate, raw, `condition:${field.name}:${predicate}`);
  };
  const createFieldAction = (field: DocumentEditorField, action: RuleLocalDocumentActionKind) => {
    const raw = rawValues[field.name];
    if (action !== "clear" && raw == null) return;
    requestAction(field.name, field.type, action, action === "clear" ? null : raw, action === "insert" ? fieldIndices[field.name] ?? 0 : null, `action:${field.name}:${action}`);
  };
  const documentConditions = conditionLeaves(props.condition).filter((condition) => condition.source === "document");
  const documentActions = props.actions.filter((action) => action.source === "document" || action.source === "record_match");
  const editorFields = documentEditorFields(props.fields, documentConditions, props.actions, props.localTypes);
  const localFields = ruleLocalFields(documentConditions, props.actions, props.fields, props.localTypes);
  const selectedLocalType = props.localTypes.find((capability) => capability.value_type === localType);
  return <div className="space-y-2"><p className="text-sm font-medium">{props.packageLabel}</p><p className="text-xs text-[var(--telemetry-muted)]">字段 {editorFields.length} 个 · 条件 {documentConditions.length} 个 · Document 动作 {documentActions.length} 个</p>
    <DocumentMetadataTree condition={props.condition} fields={props.fields} localFields={localFields} />
    <fieldset className="grid gap-2 rounded-md border border-[var(--telemetry-line)] p-2 sm:grid-cols-3">
      <legend className="px-1 text-xs font-medium">规则本地 metadata</legend>
      <TextField><Label>RFC 6901 path</Label><Input aria-label="规则本地 RFC 6901 路径" value={localPath} onChange={(event) => setLocalPath(event.target.value)} /></TextField>
      <Select aria-label="规则本地类型" selectedKey={localType || null} onSelectionChange={(key) => setLocalType(String(key) as typeof localType)}>
        <Label>类型</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
        <Select.Popover><ListBox>{props.localTypes.map((capability) => <ListBox.Item id={capability.value_type} key={capability.value_type} textValue={capability.value_type}>{capability.value_type}</ListBox.Item>)}</ListBox></Select.Popover>
      </Select>
      <TextField><Label>JSON 值</Label><Input aria-label="规则本地值" value={localValue} onChange={(event) => setLocalValue(event.target.value)} /></TextField>
      <Select aria-label="规则本地谓词" selectedKey={localPredicate || null} onSelectionChange={(key) => setLocalPredicate(String(key) as RuleLocalDocumentPredicateKind)}><Label>谓词</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{(selectedLocalType?.predicates ?? []).map((predicate) => <ListBox.Item id={predicate} key={predicate} textValue={predicate}>{predicate}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      <Select aria-label="规则本地动作" selectedKey={localAction || null} onSelectionChange={(key) => setLocalAction(String(key) as RuleLocalDocumentActionKind)}><Label>动作</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{(selectedLocalType?.actions ?? []).map((action) => <ListBox.Item id={action} key={action} textValue={action}>{action}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      {localAction === "insert" && <NumberField aria-label="规则本地 Insert index" minValue={0} value={localIndex} onChange={setLocalIndex}><Label>Index</Label><NumberField.Group><NumberField.Input /></NumberField.Group></NumberField>}
      <Button isDisabled={pending || !localType || !localPredicate || localValue === "" || localPath === ""} size="sm" variant="outline" onPress={() => localType && localPredicate && requestCondition(localPath, localType, localPredicate, localValue, "local-condition")}>创建规则本地元数据条件</Button>
      <Button isDisabled={pending || !localType || !localAction || localPath === "" || (localAction !== "clear" && localValue === "")} size="sm" variant="outline" onPress={() => localType && localAction && requestAction(localPath, localType, localAction, localAction === "clear" ? null : localValue, localAction === "insert" ? localIndex : null, "local-action")}>创建规则本地元数据动作</Button>
      <p className="text-xs text-[var(--telemetry-muted)] sm:col-span-3">路径与类型先作为可编辑草稿；创建条件后才由 Document leaf 随规则保存。</p>
    </fieldset>
    {editorFields.map((field) => <div className="space-y-2 rounded-md border p-2" key={field.name}><TextField><Label>Document 值：{field.label}</Label><Input aria-label={`Document 值：${field.label}`} value={rawValues[field.name] ?? ""} onChange={(event) => setRawValues((current) => ({ ...current, [field.name]: event.target.value }))} /></TextField>{field.actions.includes("insert") && <NumberField aria-label={`Document Insert index：${field.label}`} minValue={0} value={fieldIndices[field.name] ?? 0} onChange={(value) => setFieldIndices((current) => ({ ...current, [field.name]: value }))}><Label>Insert index</Label><NumberField.Group><NumberField.Input /></NumberField.Group></NumberField>}<div className="flex flex-wrap gap-2">
      {field.predicates.map((predicate) => <Button isDisabled={pending || rawValues[field.name] == null} key={predicate} size="sm" variant="outline" onPress={() => createPredicate(field, predicate)}>添加条件：{field.label} {predicate}</Button>)}
      {field.actions.map((action) => <Button isDisabled={pending || (action !== "clear" && rawValues[field.name] == null)} key={action} size="sm" variant="outline" onPress={() => createFieldAction(field, action)}>添加动作：{actionLabel(action)} {field.label}</Button>)}
    </div></div>)}
    <div className="flex flex-wrap gap-2">
      {props.commonActions.map((action) => <Button key={action} size="sm" variant="outline" onPress={() => void runAsync(`common:${action}`, () => callCommand(commands.ruleDefinitionDocumentCommonActionDraft(action)), props.onCreateAction, (reason) => setError(errorMessage(reason)))}>添加：{action === "record_match" ? "记录命中" : action}</Button>)}
    </div>
    {error && <p role="alert" className="text-xs text-red-600">{error}</p>}
  </div>;
}

function EditorShell({ children }: { children: React.ReactNode }) {
  return <div className="space-y-5 p-1">{children}</div>;
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
  value: Condition | HttpAction,
) {
  if (editorScope(input) !== expectedScope || input.draft.content.type !== "http") return input;
  const content = input.draft.content.value;
  const next = kind === "condition"
    ? { ...content, condition: appendCondition(content.condition, value as Condition) }
    : { ...content, actions: [...content.actions, wrapRuleAction(value as HttpAction)] };
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

function httpRuleActionLabel(action: HttpAction) {
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

function wrapRuleAction(action: HttpAction): UnifiedAction {
  return typeof action === "object" && "Terminal" in action
    ? { source: "terminal", value: action.Terminal! }
    : { source: "http", value: action };
}

function unifiedActionLabel(action: UnifiedAction) {
  if (action.source === "record_match") return "记录命中";
  if (action.source === "http") return httpRuleActionLabel(action.value);
  if (action.source === "terminal") {
    if (typeof action.value === "object" && "MockResponse" in action.value) return "Mock Response";
    return httpRuleActionLabel({ Terminal: action.value });
  }
  const mutation: DocumentMutation = action.value;
  if (mutation.type === "clear") return `清除字段 ${mutation.path}`;
  if (mutation.type === "insert") return `插入 ${mutation.path}[${mutation.index}]`;
  if (mutation.type === "append") return `追加到 ${mutation.path}`;
  return `设置字段 ${mutation.path}`;
}

function ruleCondition(input: RuleDefinitionSaveInput): ConditionTree {
  return input.draft.content.value.condition;
}

function ruleActions(input: RuleDefinitionSaveInput): UnifiedAction[] {
  return input.draft.content.value.actions;
}

function updateRuleCondition(input: RuleDefinitionSaveInput, condition: ConditionTree): RuleDefinitionSaveInput {
  const content = input.draft.content;
  return content.type === "http"
    ? { ...input, draft: { ...input.draft, content: { type: "http", value: { ...content.value, condition } } } }
    : { ...input, draft: { ...input.draft, content: { type: "socket", value: { ...content.value, condition } } } };
}

function updateRuleActions(input: RuleDefinitionSaveInput, actions: UnifiedAction[]): RuleDefinitionSaveInput {
  const content = input.draft.content;
  return content.type === "http"
    ? { ...input, draft: { ...input.draft, content: { type: "http", value: { ...content.value, actions } } } }
    : { ...input, draft: { ...input.draft, content: { type: "socket", value: { ...content.value, actions } } } };
}

function actionLabel(action: RuleLocalDocumentActionKind) {
  return action.charAt(0).toUpperCase() + action.slice(1);
}
