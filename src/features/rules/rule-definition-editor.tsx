import { useState } from "react";
import { Button, Input, Label, ListBox, NumberField, Select, Switch, TextArea, TextField } from "@heroui/react";
import type {
  Condition,
  ConditionTree,
  DocumentMutation,
  HttpRuleEditorStageViewModel,
  RuleCommonActionCapability,
  ProxyListener,
  HttpAction,
  RuleActionKind,
  RuleActionCapabilityViewModel,
  RuleDocumentConditionPathCapability,
  RuleDocumentActionCapability,
  RuleDocumentSchemaFieldCapability,
  RuleDefinitionSaveInput,
  RuleEditorContext,
  RuleLocalDocumentActionKind,
  RuleLocalDocumentPredicateKind,
  RuleLocalDocumentTypeCapability,
  RuleLocalDocumentValueType,
  RuleMatchFieldCapabilityViewModel,
  RuleMatchFieldKind,
  RuleMatchOperatorKind,
  SocketRuleEditorStageViewModel,
  UnifiedAction,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { httpActionLabel, matchFieldLabel, ruleActionKindLabel, ruleStageIncompatibility, ruleStageLabel } from "./rule-definition-model";
import { documentEditorFields, ruleLocalFields, type DocumentEditorField } from "./rule-document-fields";
import { documentSchemaFields, type DocumentSchemaField } from "./rule-document-schema";
import { ConditionTreeEditor, DocumentMetadataTree, OrderedActionList } from "./rule-tree-editors";
import { useAsyncRequestSlots } from "./use-async-request-slots";

type EditorStage = HttpRuleEditorStageViewModel | SocketRuleEditorStageViewModel;
type RuleDefinitionChange = RuleDefinitionSaveInput | ((current: RuleDefinitionSaveInput) => RuleDefinitionSaveInput);
type ConditionInsertion = {
  scope: string;
  targetPath: number[];
  subgroup: "all" | "any" | null;
};

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
  const [conditionInsertion, setConditionInsertion] = useState<ConditionInsertion>({ scope: "", targetPath: [], subgroup: null });
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
  const conditionTreeReason = conditionTreeInvalid(ruleCondition(input))
    ? "条件树不能为空，请通过下方 Rust 条件工厂添加第一个条件。"
    : null;
  const updateDraft = (draft: RuleDefinitionSaveInput["draft"]) => props.onChange({ ...input, draft });
  const scope = editorScope(input);
  const activeInsertion = conditionInsertion.scope === scope
    ? conditionInsertion
    : { scope, targetPath: [], subgroup: null };

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
      {conditionTreeReason && <p role="alert" className="text-sm text-red-600">{conditionTreeReason}</p>}
      {input.draft.content.type === "http" ? (
        <HttpContentEditor conditionInsertion={activeInsertion} conditionPath={props.context?.document_condition_path} input={input} localTypes={props.context?.local_document_types ?? []} onChange={props.onChange} stage={stage && "http" in stage ? stage : undefined} />
      ) : (
        <SocketContentEditor conditionInsertion={activeInsertion} conditionPath={props.context?.document_condition_path} input={input} localTypes={props.context?.local_document_types ?? []} onChange={props.onChange} stage={stage && !("http" in stage) ? stage : undefined} />
      )}
      <ConditionTreeEditor
        key={scope}
        tree={ruleCondition(input)}
        onChange={(condition) => props.onChange(updateRuleCondition(input, condition))}
        onInsertRequest={(targetPath, subgroup) => setConditionInsertion({ scope, targetPath, subgroup })}
      />
      <OrderedActionList actions={ruleActions(input)} label={unifiedActionLabel} onChange={(actions) => props.onChange(updateRuleActions(input, actions))} />
      {Object.values(props.fieldErrors).flat().length > 0 && <p role="alert" className="text-sm text-red-600">{Object.values(props.fieldErrors).flat().join("；")}</p>}
      <div className="flex gap-2">
        <Button isDisabled={props.pending || input.draft.name.trim() === "" || currentStageReason != null || conditionTreeReason != null} variant="primary" onPress={props.onSave}>保存规则</Button>
        {existing && <Button isDisabled={props.pending} variant="outline" onPress={props.onCopy}>复制规则</Button>}
        {existing && <Button isDisabled={props.pending} variant="danger-soft" onPress={() => setConfirmingDelete(true)}>删除规则</Button>}
      </div>
      {confirmingDelete && <div className="rounded-lg border border-red-500 p-3" role="alertdialog"><p>删除后无法恢复。</p><div className="mt-2 flex gap-2"><Button variant="danger" onPress={props.onDelete}>确认删除</Button><Button variant="outline" onPress={() => setConfirmingDelete(false)}>取消</Button></div></div>}
    </EditorShell>
  );
}

function HttpContentEditor(props: { conditionInsertion: ConditionInsertion; conditionPath?: RuleDocumentConditionPathCapability; input: RuleDefinitionSaveInput; stage?: HttpRuleEditorStageViewModel; localTypes: RuleLocalDocumentTypeCapability[]; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "http") return null;
  const value = props.input.draft.content.value;
  const scope = editorScope(props.input);
  const update = (next: typeof value) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "http", value: next } } });
  return <section className="space-y-4"><h3 className="font-semibold">HTTP 规则内容</h3>
    <TextField><Label>说明</Label><TextArea className="min-h-20" value={value.description} onChange={(event) => update({ ...value, description: event.target.value })} /></TextField>
    <section className="space-y-2 rounded-lg border border-[var(--telemetry-line)] p-3"><h4 className="font-medium">HTTP Header、URL 与请求信息</h4>
      <p className="text-xs text-[var(--telemetry-muted)]">条件 {conditionLeafCount(value.condition)} 个 · 动作 {value.actions.length} 个</p>
      <CapabilityList labels={[
        ...(props.stage?.http?.match_fields ?? []).map((field) => matchFieldLabel(field.kind)),
        ...(props.stage?.http?.actions ?? []).map((action) => ruleActionKindLabel(action.kind)),
      ]} />
      {props.stage?.http && <HttpFactoryControls
        actionCapabilities={props.stage.http.actions}
        actions={value.actions}
        condition={value.condition}
        editorScope={scope}
        key={scope}
        onChange={(condition, actions) => update({ ...value, condition, actions })}
        onCreateAction={(action) => props.onChange((current) => appendHttpFactoryResult(current, scope, "action", action))}
        onCreateCondition={(condition) => props.onChange((current) => appendHttpFactoryResult(current, scope, "condition", condition, props.conditionInsertion))}
        matchFields={props.stage.http.match_fields}
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
          conditionPath={props.conditionPath}
          fields={props.stage?.document_fields ?? []}
          localTypes={props.localTypes}
          key={`${scope}:document`}
          packageLabel={`${value.document.package.id}@${value.document.package.version}`}
          onActionsChange={(actions) => update({ ...value, actions })}
          onConditionChange={(condition) => update({ ...value, condition })}
          onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, scope, "action", action))}
          onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, scope, "condition", condition, props.conditionInsertion))}
        />
      </> : <OptionalHttpDocument stage={props.stage} onAdd={(document) => update({ ...value, document })} />}
    </section>
  </section>;
}

function HttpFactoryControls(props: {
  actionCapabilities: RuleActionCapabilityViewModel[];
  actions: UnifiedAction[];
  condition: ConditionTree;
  editorScope: string;
  matchFields: RuleMatchFieldCapabilityViewModel[];
  stage: import("@/generated/rust-types").MessageStage;
  onChange: (condition: ConditionTree, actions: UnifiedAction[]) => void;
  onCreateAction: (action: HttpAction) => void;
  onCreateCondition: (condition: Condition) => void;
}) {
  const [error, setError] = useState<string>();
  const [fieldKind, setFieldKind] = useState<RuleMatchFieldKind | "">("");
  const selectedField = props.matchFields.find((field) => field.kind === fieldKind);
  const [operatorKind, setOperatorKind] = useState<RuleMatchOperatorKind | "">("");
  const [selector, setSelector] = useState("");
  const [value, setValue] = useState("");
  const [nthCount, setNthCount] = useState("");
  const [actionKind, setActionKind] = useState<RuleActionKind | "">("");
  const selectedAction = props.actionCapabilities.find((action) => action.kind === actionKind);
  const [actionParameters, setActionParameters] = useState("");
  const { runAsync } = useAsyncRequestSlots(props.editorScope);
  function addHttpCondition() {
    if (!fieldKind || !operatorKind) return;
    setError(undefined);
    return runAsync(
      "condition",
      () => callCommand(commands.ruleDefinitionHttpConditionDraft(fieldKind, selectedField?.selector ? selector : null, operatorKind, value, props.stage)),
      props.onCreateCondition,
      (reason) => setError(errorMessage(reason)),
    );
  }
  function addNthHitCondition() {
    const count = Number(nthCount);
    if (!Number.isSafeInteger(count) || count <= 0) return;
    setError(undefined);
    return runAsync("condition", () => callCommand(commands.ruleDefinitionNthHitConditionDraft({ count })), props.onCreateCondition, (reason) => setError(errorMessage(reason)));
  }
  function addAction(capability: RuleActionCapabilityViewModel) {
    const parametersJson = capability.parameters_required ? actionParameters : null;
    if (parametersJson !== null && parametersJson.trim() === "") return;
    setError(undefined);
    return runAsync(
      "action",
      () => callCommand(commands.ruleDefinitionActionDraft({ kind: capability.kind, parameters_json: parametersJson }, props.stage)),
      props.onCreateAction,
      (reason) => setError(errorMessage(reason)),
    );
  }
  return <div className="space-y-2">
    {props.matchFields.length > 0 && <div className="grid gap-2 sm:grid-cols-2">
      <Select aria-label="HTTP 匹配字段" selectedKey={fieldKind || null} onSelectionChange={(key) => {
        const next = String(key) as RuleMatchFieldKind;
        setFieldKind(next);
        setOperatorKind("");
        setSelector("");
      }}><Label>HTTP 匹配字段</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{props.matchFields.map((field) => <ListBox.Item id={field.kind} key={field.kind} textValue={matchFieldLabel(field.kind)}>{matchFieldLabel(field.kind)}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      <Select aria-label="HTTP 匹配操作符" selectedKey={operatorKind || null} onSelectionChange={(key) => setOperatorKind(String(key) as RuleMatchOperatorKind)}><Label>HTTP 匹配操作符</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{(selectedField?.operators ?? []).map((operator) => <ListBox.Item id={operator} key={operator} textValue={operator}>{operator}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      {selectedField?.selector === "header_name_pointer" && <TextField><Label>Header selector（/name）</Label><Input aria-label="Header selector（/name）" value={selector} onChange={(event) => setSelector(event.target.value)} /></TextField>}
      <TextField><Label>HTTP 匹配值</Label><Input aria-label="HTTP 匹配值" value={value} onChange={(event) => setValue(event.target.value)} /></TextField>
    </div>}
    {operatorKind === "wildcard" && <p className="text-xs text-[var(--telemetry-muted)]">Wildcard 仅用于条件匹配；表达式由 Rust 校验。</p>}
    <div className="flex flex-wrap gap-1">
      {props.matchFields.length > 0 && <Button isDisabled={!fieldKind || !operatorKind || value === "" || (selectedField?.selector != null && selector === "")} size="sm" variant="outline" onPress={() => void addHttpCondition()}>创建 HTTP 条件</Button>}
      <TextField><Label>第 N 次命中</Label><Input aria-label="第 N 次命中" inputMode="numeric" value={nthCount} onChange={(event) => setNthCount(event.target.value)} /></TextField>
      <Button isDisabled={!Number.isSafeInteger(Number(nthCount)) || Number(nthCount) <= 0} size="sm" variant="outline" onPress={() => void addNthHitCondition()}>添加条件：第 N 次命中</Button>
    </div>
    {props.actionCapabilities.length > 0 && <div className="grid gap-2 sm:grid-cols-2">
      <Select aria-label="HTTP 动作类型" selectedKey={actionKind || null} onSelectionChange={(key) => { setActionKind(String(key) as RuleActionKind); setActionParameters(""); }}><Label>HTTP 动作类型</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{props.actionCapabilities.map(({ kind }) => <ListBox.Item id={kind} key={kind} textValue={ruleActionKindLabel(kind)}>{ruleActionKindLabel(kind)}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      {selectedAction?.parameters_required && <TextField><Label>动作参数 JSON</Label><TextArea aria-label="动作参数 JSON" value={actionParameters} onChange={(event) => setActionParameters(event.target.value)} /></TextField>}
      <Button isDisabled={!selectedAction || (selectedAction.parameters_required && actionParameters.trim() === "")} size="sm" variant="outline" onPress={() => selectedAction && void addAction(selectedAction)}>创建 HTTP 动作</Button>
    </div>}
    {error && <p role="alert" className="text-xs text-red-600">{error}</p>}
  </div>;
}

function SocketContentEditor(props: { conditionInsertion: ConditionInsertion; conditionPath?: RuleDocumentConditionPathCapability; input: RuleDefinitionSaveInput; stage?: SocketRuleEditorStageViewModel; localTypes: RuleLocalDocumentTypeCapability[]; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "socket") return null;
  const value = props.input.draft.content.value;
  return <section className="space-y-3"><h3 className="font-semibold">Socket Document 规则内容</h3>
    <DocumentEditor actions={value.actions} commonActions={props.stage?.common_actions ?? []} condition={value.condition} conditionPath={props.conditionPath} editorScope={editorScope(props.input)} fields={props.stage?.document_fields ?? []} localTypes={props.localTypes} key={`${editorScope(props.input)}:document`} packageLabel={`${value.package.id}@${value.package.version}`} onActionsChange={(actions) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, actions } } } })} onConditionChange={(condition) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, condition } } } })} onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "action", action))} onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "condition", condition, props.conditionInsertion))} />
  </section>;
}

function OptionalHttpDocument(props: { stage?: HttpRuleEditorStageViewModel; onAdd: (document: NonNullable<import("@/generated/rust-types").HttpRuleContent["document"]>) => void }) {
  const content = props.stage?.new_rule_draft.content;
  const document = content?.type === "http" ? content.value.document : null;
  if (!document) return <p className="text-sm text-[var(--telemetry-muted)]">当前 Listener/阶段没有协议 Body Document 能力。</p>;
  return <div className="space-y-2"><p className="text-sm text-[var(--telemetry-muted)]">当前规则仅处理 HTTP Header；可按 Rust 草稿启用 Body Document。</p><Button size="sm" variant="outline" onPress={() => props.onAdd(document)}>添加 HTTP Body Document</Button></div>;
}

function DocumentEditor(props: { packageLabel: string; editorScope: string; fields: RuleDocumentSchemaFieldCapability[]; conditionPath?: RuleDocumentConditionPathCapability; localTypes: RuleLocalDocumentTypeCapability[]; commonActions: RuleCommonActionCapability[]; condition: ConditionTree; actions: UnifiedAction[]; onCreateCondition: (condition: Condition) => void; onCreateAction: (action: UnifiedAction) => void; onConditionChange: (condition: ConditionTree) => void; onActionsChange: (actions: UnifiedAction[]) => void }) {
  const [rawValues, setRawValues] = useState<Record<string, string>>({});
  const [fieldIndices, setFieldIndices] = useState<Record<string, number>>({});
  const [localPath, setLocalPath] = useState("");
  const [schemaPath, setSchemaPath] = useState<string | null>(null);
  const [manualPathSelected, setManualPathSelected] = useState(false);
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
  const createFieldAction = (field: DocumentEditorField, action: RuleDocumentActionCapability) => {
    const raw = rawValues[field.name];
    if (action.kind !== "clear" && raw == null) return;
    const valueType = action.operand_value_type ?? action.target_value_type;
    requestAction(field.name, valueType, action.kind, action.kind === "clear" ? null : raw, action.kind === "insert" ? fieldIndices[field.name] ?? 0 : null, `action:${field.name}:${action.kind}`);
  };
  const documentConditions = conditionLeaves(props.condition).filter((condition): condition is Extract<Condition, { source: "document" | "document_pattern" }> => condition.source === "document" || condition.source === "document_pattern");
  const documentActions = props.actions.filter((action) => action.source === "document" || action.source === "record_match");
  const schemaFields = documentSchemaFields(props.fields);
  const editorFields = documentEditorFields(schemaFields, documentConditions, props.actions, props.localTypes);
  const localFields = ruleLocalFields(documentConditions, props.actions, schemaFields, props.localTypes);
  const selectedSchemaField = schemaPath === null ? undefined : schemaFields.find((field) => field.name === schemaPath);
  const selectedLocalType = props.localTypes.find((capability) => capability.value_type === localType);
  const selectedPredicates = selectedSchemaField?.predicates ?? selectedLocalType?.predicates ?? [];
  const selectedActions = selectedSchemaField?.actions ?? selectedLocalType?.actions ?? [];
  const selectedLocalAction = selectedActions.find((action) => action.kind === localAction);
  const wildcard = props.conditionPath?.wildcard_token;
  const actionPathExact = !wildcard || !localPath.split("/").includes(wildcard);
  return <div className="space-y-2"><p className="text-sm font-medium">{props.packageLabel}</p><p className="text-xs text-[var(--telemetry-muted)]">字段 {editorFields.length} 个 · 条件 {documentConditions.length} 个 · Document 动作 {documentActions.length} 个</p>
    <DocumentMetadataTree condition={props.condition} fields={schemaFields} localFields={localFields} />
    <fieldset className="grid gap-2 rounded-md border border-[var(--telemetry-line)] p-2 sm:grid-cols-3">
      <legend className="px-1 text-xs font-medium">规则本地 metadata</legend>
      {schemaFields.length > 0 && <Select aria-label="Document Schema 条件路径" selectedKey={schemaPath === null ? null : schemaSelectionKey(schemaPath)} onSelectionChange={(key) => {
        const field = schemaFields.find((item) => schemaSelectionKey(item.name) === String(key));
        if (!field) return;
        const path = field.name;
        setSchemaPath(path);
        setManualPathSelected(false);
        setLocalPath(path);
        setLocalType(field.type);
      }}><Label>Document Schema 条件路径</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{schemaFields.map((field) => <ListBox.Item id={schemaSelectionKey(field.name)} key={schemaSelectionKey(field.name)} textValue={schemaPathLabel(field)}>{schemaPathLabel(field)}</ListBox.Item>)}</ListBox></Select.Popover></Select>}
      <TextField><Label>手动 Document 条件路径</Label><Input aria-label="手动 Document 条件路径" value={localPath} onChange={(event) => { setSchemaPath(null); setManualPathSelected(true); setLocalPath(event.target.value); }} /></TextField>
      <Button size="sm" variant="outline" onPress={() => { setSchemaPath(null); setManualPathSelected(true); setLocalPath(""); }}>手动选择根路径 /</Button>
      <Select aria-label="规则本地类型" isDisabled={schemaPath !== null} selectedKey={localType || null} onSelectionChange={(key) => setLocalType(String(key) as typeof localType)}>
        <Label>类型</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
        <Select.Popover><ListBox>{props.localTypes.map((capability) => <ListBox.Item id={capability.value_type} key={capability.value_type} textValue={capability.value_type}>{capability.value_type}</ListBox.Item>)}</ListBox></Select.Popover>
      </Select>
      <TextField><Label>JSON 值</Label><Input aria-label="规则本地值" value={localValue} onChange={(event) => setLocalValue(event.target.value)} /></TextField>
      <Select aria-label="规则本地谓词" selectedKey={localPredicate || null} onSelectionChange={(key) => setLocalPredicate(String(key) as RuleLocalDocumentPredicateKind)}><Label>谓词</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{selectedPredicates.map((predicate) => <ListBox.Item id={predicate} key={predicate} textValue={predicate}>{predicate}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      <Select aria-label="规则本地动作" selectedKey={localAction || null} onSelectionChange={(key) => setLocalAction(String(key) as RuleLocalDocumentActionKind)}><Label>动作</Label><Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{selectedActions.map((action) => <ListBox.Item id={action.kind} key={action.kind} textValue={action.kind}>{action.kind}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      {localAction === "insert" && <NumberField aria-label="规则本地 Insert index" minValue={0} value={localIndex} onChange={setLocalIndex}><Label>Index</Label><NumberField.Group><NumberField.Input /></NumberField.Group></NumberField>}
      <Button isDisabled={pending || !localType || !localPredicate || localValue === "" || (schemaPath === null && !manualPathSelected)} size="sm" variant="outline" onPress={() => localType && localPredicate && requestCondition(localPath, localType, localPredicate, localValue, "local-condition")}>创建规则本地元数据条件</Button>
      <Button isDisabled={pending || !actionPathExact || !selectedLocalAction || (schemaPath === null && !manualPathSelected) || (localAction !== "clear" && localValue === "")} size="sm" variant="outline" onPress={() => selectedLocalAction && requestAction(localPath, selectedLocalAction.operand_value_type ?? selectedLocalAction.target_value_type, selectedLocalAction.kind, selectedLocalAction.kind === "clear" ? null : localValue, selectedLocalAction.kind === "insert" ? localIndex : null, "local-action")}>创建规则本地元数据动作</Button>
      {props.conditionPath && <p className="text-xs text-[var(--telemetry-muted)] sm:col-span-3">{props.conditionPath.wildcard_token} 仅匹配一层；展开多个节点时按 ANY 匹配。</p>}
      <p className="text-xs text-[var(--telemetry-muted)] sm:col-span-3">Wildcard 仅用于条件；Set/Clear/Insert/Append 路径必须是精确 RFC 6901。</p>
    </fieldset>
    {editorFields.map((field) => <div className="space-y-2 rounded-md border p-2" key={field.name}><TextField><Label>Document 值：{field.label}</Label><Input aria-label={`Document 值：${field.label}`} value={rawValues[field.name] ?? ""} onChange={(event) => setRawValues((current) => ({ ...current, [field.name]: event.target.value }))} /></TextField>{field.actions.some((action) => action.kind === "insert") && <NumberField aria-label={`Document Insert index：${field.label}`} minValue={0} value={fieldIndices[field.name] ?? 0} onChange={(value) => setFieldIndices((current) => ({ ...current, [field.name]: value }))}><Label>Insert index</Label><NumberField.Group><NumberField.Input /></NumberField.Group></NumberField>}<div className="flex flex-wrap gap-2">
      {field.predicates.map((predicate) => <Button isDisabled={pending || rawValues[field.name] == null} key={predicate} size="sm" variant="outline" onPress={() => createPredicate(field, predicate)}>添加条件：{field.label} {predicate}</Button>)}
      {field.actions.map((action) => <Button isDisabled={pending || (action.kind !== "clear" && rawValues[field.name] == null)} key={action.kind} size="sm" variant="outline" onPress={() => createFieldAction(field, action)}>添加动作：{actionLabel(action.kind)} {field.label}</Button>)}
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

function schemaSelectionKey(path: string) {
  return `pointer:${path}`;
}

function schemaPathLabel(field: DocumentSchemaField) {
  if (field.name === "") return `${field.label} · /（根）`;
  if (field.name === "/") return `${field.label} · /（空名称属性）`;
  return `${field.label} · ${field.name}`;
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
  insertion?: ConditionInsertion,
) {
  if (editorScope(input) !== expectedScope || input.draft.content.type !== "http") return input;
  const content = input.draft.content.value;
  const next = kind === "condition"
    ? { ...content, condition: appendCondition(content.condition, value as Condition, insertion) }
    : { ...content, actions: [...content.actions, wrapRuleAction(value as HttpAction)] };
  return { ...input, draft: { ...input.draft, content: { type: "http" as const, value: next } } };
}

function appendDocumentResult(input: RuleDefinitionSaveInput, expectedScope: string, kind: "condition" | "action", value: Condition | UnifiedAction, insertion?: ConditionInsertion) {
  if (editorScope(input) !== expectedScope) return input;
  const content = input.draft.content;
  if (content.type === "http") {
    if (!content.value.document) return input;
    const next = kind === "condition"
      ? { ...content.value, condition: appendCondition(content.value.condition, value as Condition, insertion) }
      : { ...content.value, actions: [...content.value.actions, value as UnifiedAction] };
    return { ...input, draft: { ...input.draft, content: { type: "http" as const, value: next } } };
  }
  const next = kind === "condition"
    ? { ...content.value, condition: appendCondition(content.value.condition, value as Condition, insertion) }
    : { ...content.value, actions: [...content.value.actions, value as UnifiedAction] };
  return { ...input, draft: { ...input.draft, content: { type: "socket" as const, value: next } } };
}

function CapabilityList({ labels }: { labels: string[] }) {
  return labels.length > 0 ? <div className="flex flex-wrap gap-1">{labels.map((label) => <span className="rounded-full bg-[var(--telemetry-soft)] px-2 py-1 text-xs" key={label}>{label}</span>)}</div> : <p className="text-xs text-[var(--telemetry-muted)]">Rust 未声明此阶段的 HTTP 能力。</p>;
}

function appendCondition(tree: ConditionTree, condition: Condition, insertion?: ConditionInsertion): ConditionTree {
  const leaf: ConditionTree = { operator: "leaf", children: condition };
  if (insertion) return insertCondition(tree, insertion.targetPath, insertion.subgroup, leaf);
  return tree.operator === "all"
    ? { ...tree, children: [...tree.children, leaf] }
    : { operator: "all", children: [tree, leaf] };
}

function insertCondition(tree: ConditionTree, targetPath: number[], subgroup: "all" | "any" | null, leaf: ConditionTree): ConditionTree {
  if (targetPath.length === 0) {
    const inserted = subgroup == null ? leaf : { operator: subgroup, children: [leaf] } as ConditionTree;
    if (tree.operator === "leaf") return { operator: "all", children: [tree, inserted] };
    return { ...tree, children: [...tree.children, inserted] };
  }
  if (tree.operator === "leaf") return tree;
  const [index, ...rest] = targetPath;
  const child = tree.children[index];
  if (!child) return tree;
  const updated = insertCondition(child, rest, subgroup, leaf);
  if (updated === child) return tree;
  return { ...tree, children: tree.children.map((item, itemIndex) => itemIndex === index ? updated : item) };
}

function conditionLeaves(tree: ConditionTree): Condition[] {
  return tree.operator === "leaf" ? [tree.children] : tree.children.flatMap(conditionLeaves);
}

function conditionLeafCount(tree: ConditionTree) {
  return conditionLeaves(tree).length;
}

function conditionTreeInvalid(tree: ConditionTree): boolean {
  return tree.operator !== "leaf" && (tree.children.length === 0 || tree.children.some(conditionTreeInvalid));
}

function wrapRuleAction(action: HttpAction): UnifiedAction {
  return typeof action === "object" && "Terminal" in action
    ? { source: "terminal", value: action.Terminal! }
    : { source: "http", value: action };
}

function unifiedActionLabel(action: UnifiedAction) {
  if (action.source === "record_match") return "记录命中";
  if (action.source === "http") return httpActionLabel(action.value);
  if (action.source === "terminal") {
    if (typeof action.value === "object" && "MockResponse" in action.value) return "Mock Response";
    return httpActionLabel({ Terminal: action.value });
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
