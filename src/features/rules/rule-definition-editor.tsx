import { useState } from "react";
import { Button, Input, Label, ListBox, NumberField, Select, Switch, TextArea, TextField } from "@heroui/react";
import type {
  Condition,
  DocumentMutation,
  HttpRuleEditorStageViewModel,
  RuleCommonActionCapability,
  ProxyListener,
  HttpAction,
  RuleActionKind,
  RuleActionCapabilityViewModel,
  RuleDocumentConditionPathCapability,
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
import { documentSchemaFields, type DocumentSchemaField } from "./rule-document-schema";
import { FlatConditionList, OrderedActionList } from "./rule-list-editors";
import { useAsyncRequestSlots } from "./use-async-request-slots";

type EditorStage = HttpRuleEditorStageViewModel | SocketRuleEditorStageViewModel;
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
  const conditionsReason = ruleConditions(input).length === 0
    ? "至少需要一个条件，请通过下方 Rust 条件工厂添加。"
    : null;
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
      {conditionsReason && <p role="alert" className="text-sm text-red-600">{conditionsReason}</p>}
      {input.draft.content.type === "http" ? (
        <HttpContentEditor conditionPath={props.context?.document_condition_path} input={input} localTypes={props.context?.local_document_types ?? []} onChange={props.onChange} stage={stage && "http" in stage ? stage : undefined} />
      ) : (
        <SocketContentEditor conditionPath={props.context?.document_condition_path} input={input} localTypes={props.context?.local_document_types ?? []} onChange={props.onChange} stage={stage && !("http" in stage) ? stage : undefined} />
      )}
      <FlatConditionList conditions={ruleConditions(input)} onChange={(conditions) => props.onChange(updateRuleConditions(input, conditions))} />
      <OrderedActionList actions={ruleActions(input)} label={unifiedActionLabel} onChange={(actions) => props.onChange(updateRuleActions(input, actions))} />
      {Object.values(props.fieldErrors).flat().length > 0 && <p role="alert" className="text-sm text-red-600">{Object.values(props.fieldErrors).flat().join("；")}</p>}
      <div className="flex gap-2">
        <Button isDisabled={props.pending || input.draft.name.trim() === "" || currentStageReason != null || conditionsReason != null} variant="primary" onPress={props.onSave}>保存规则</Button>
        {existing && <Button isDisabled={props.pending} variant="outline" onPress={props.onCopy}>复制规则</Button>}
        {existing && <Button isDisabled={props.pending} variant="danger-soft" onPress={() => setConfirmingDelete(true)}>删除规则</Button>}
      </div>
      {confirmingDelete && <div className="rounded-lg border border-red-500 p-3" role="alertdialog"><p>删除后无法恢复。</p><div className="mt-2 flex gap-2"><Button variant="danger" onPress={props.onDelete}>确认删除</Button><Button variant="outline" onPress={() => setConfirmingDelete(false)}>取消</Button></div></div>}
    </EditorShell>
  );
}

function HttpContentEditor(props: { conditionPath?: RuleDocumentConditionPathCapability; input: RuleDefinitionSaveInput; stage?: HttpRuleEditorStageViewModel; localTypes: RuleLocalDocumentTypeCapability[]; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "http") return null;
  const value = props.input.draft.content.value;
  const scope = editorScope(props.input);
  const update = (next: typeof value) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "http", value: next } } });
  return <section className="space-y-4"><h3 className="font-semibold">HTTP 规则内容</h3>
    <TextField><Label>说明</Label><TextArea className="min-h-20" value={value.description} onChange={(event) => update({ ...value, description: event.target.value })} /></TextField>
    <section className="space-y-2 rounded-lg border border-[var(--telemetry-line)] p-3"><h4 className="font-medium">HTTP Header、URL 与请求信息</h4>
      <p className="text-xs text-[var(--telemetry-muted)]">条件 {value.conditions.length} 个 · 动作 {value.actions.length} 个</p>
      <CapabilityList labels={[
        ...(props.stage?.http?.match_fields ?? []).map((field) => matchFieldLabel(field.kind)),
        ...(props.stage?.http?.actions ?? []).map((action) => ruleActionKindLabel(action.kind)),
      ]} />
      {props.stage?.http && <HttpFactoryControls
        actionCapabilities={props.stage.http.actions}
        actions={value.actions}
        conditions={value.conditions}
        editorScope={scope}
        key={scope}
        onChange={(conditions, actions) => update({ ...value, conditions, actions })}
        onCreateAction={(action) => props.onChange((current) => appendHttpFactoryResult(current, scope, "action", action))}
        onCreateCondition={(condition) => props.onChange((current) => appendHttpFactoryResult(current, scope, "condition", condition))}
        matchFields={props.stage.http.match_fields}
        stage={props.stage.http.stage}
      />}
    </section>
    <section className="space-y-2 rounded-lg border border-[var(--telemetry-line)] p-3"><h4 className="font-medium">HTTP Body Document</h4>
      <DocumentEditor
        actions={value.actions}
        commonActions={props.stage?.document_common_actions ?? []}
        conditions={value.conditions}
        editorScope={scope}
        conditionPath={props.conditionPath}
        fields={props.stage?.document_fields ?? []}
        localTypes={props.localTypes}
        key={`${scope}:document`}
        packageLabel={httpDocumentLabel(props.stage)}
        onActionsChange={(actions) => update({ ...value, actions })}
        onConditionsChange={(conditions) => update({ ...value, conditions })}
        onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, scope, "action", action))}
        onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, scope, "condition", condition))}
      />
    </section>
  </section>;
}

function HttpFactoryControls(props: {
  actionCapabilities: RuleActionCapabilityViewModel[];
  actions: UnifiedAction[];
  conditions: Condition[];
  editorScope: string;
  matchFields: RuleMatchFieldCapabilityViewModel[];
  stage: import("@/generated/rust-types").RuleStage;
  onChange: (conditions: Condition[], actions: UnifiedAction[]) => void;
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
    {props.matchFields.length > 0 && <div className="space-y-3 rounded-md border border-[var(--telemetry-line)] p-3" data-testid="http-condition-factory">
      <div className="grid items-end gap-3 sm:grid-cols-2">
        <Select aria-label="HTTP 匹配字段" selectedKey={fieldKind || null} onSelectionChange={(key) => {
          const next = String(key) as RuleMatchFieldKind;
          setFieldKind(next);
          setOperatorKind("");
          setSelector("");
        }}><Label>HTTP 匹配字段</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{props.matchFields.map((field) => <ListBox.Item id={field.kind} key={field.kind} textValue={matchFieldLabel(field.kind)}>{matchFieldLabel(field.kind)}</ListBox.Item>)}</ListBox></Select.Popover></Select>
        <Select aria-label="HTTP 匹配操作符" selectedKey={operatorKind || null} onSelectionChange={(key) => setOperatorKind(String(key) as RuleMatchOperatorKind)}><Label>HTTP 匹配操作符</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{(selectedField?.operators ?? []).map((operator) => <ListBox.Item id={operator} key={operator} textValue={operator}>{operator}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      </div>
      <div className="grid items-end gap-3 sm:grid-cols-2">
        {selectedField?.selector === "header_name_pointer" && <TextField><Label>Header selector（/name）</Label><Input aria-label="Header selector（/name）" className="h-10 w-full py-0" value={selector} onChange={(event) => setSelector(event.target.value)} /></TextField>}
        <TextField><Label>HTTP 匹配值</Label><Input aria-label="HTTP 匹配值" className="h-10 w-full py-0" value={value} onChange={(event) => setValue(event.target.value)} /></TextField>
        <Button className="h-10 w-full" isDisabled={!fieldKind || !operatorKind || value === "" || (selectedField?.selector != null && selector === "")} variant="outline" onPress={() => void addHttpCondition()}>创建 HTTP 条件</Button>
      </div>
    </div>}
    {operatorKind === "wildcard" && <p className="text-xs text-[var(--telemetry-muted)]">Wildcard 仅用于条件匹配；表达式由 Rust 校验。</p>}
    <div className="grid items-end gap-3 rounded-md border border-[var(--telemetry-line)] p-3 sm:grid-cols-2" data-testid="nth-condition-factory">
      <TextField><Label>第 N 次命中</Label><Input aria-label="第 N 次命中" className="h-10 w-full py-0" inputMode="numeric" value={nthCount} onChange={(event) => setNthCount(event.target.value)} /></TextField>
      <Button className="h-10 w-full" isDisabled={!Number.isSafeInteger(Number(nthCount)) || Number(nthCount) <= 0} variant="outline" onPress={() => void addNthHitCondition()}>添加条件：第 N 次命中</Button>
    </div>
    {props.actionCapabilities.length > 0 && <div className="space-y-3 rounded-md border border-[var(--telemetry-line)] p-3" data-testid="http-action-factory">
      <div className="grid items-end gap-3 sm:grid-cols-2" data-testid="http-action-controls">
        <Select aria-label="HTTP 动作类型" selectedKey={actionKind || null} onSelectionChange={(key) => { setActionKind(String(key) as RuleActionKind); setActionParameters(""); }}><Label>HTTP 动作类型</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{props.actionCapabilities.map(({ kind }) => <ListBox.Item id={kind} key={kind} textValue={ruleActionKindLabel(kind)}>{ruleActionKindLabel(kind)}</ListBox.Item>)}</ListBox></Select.Popover></Select>
        <Button className="h-10 w-full" isDisabled={!selectedAction || (selectedAction.parameters_required && actionParameters.trim() === "")} variant="outline" onPress={() => selectedAction && void addAction(selectedAction)}>创建 HTTP 动作</Button>
      </div>
      {selectedAction?.parameters_required && <div className="w-full" data-testid="http-action-parameters"><TextField><Label>动作参数 JSON</Label><TextArea aria-label="动作参数 JSON" className="min-h-24 w-full" value={actionParameters} onChange={(event) => setActionParameters(event.target.value)} /></TextField></div>}
    </div>}
    {error && <p role="alert" className="text-xs text-red-600">{error}</p>}
  </div>;
}

function SocketContentEditor(props: { conditionPath?: RuleDocumentConditionPathCapability; input: RuleDefinitionSaveInput; stage?: SocketRuleEditorStageViewModel; localTypes: RuleLocalDocumentTypeCapability[]; onChange: (change: RuleDefinitionChange) => void }) {
  if (props.input.draft.content.type !== "socket") return null;
  const value = props.input.draft.content.value;
  return <section className="space-y-3"><h3 className="font-semibold">Socket Document 规则内容</h3>
    <DocumentEditor actions={value.actions} commonActions={props.stage?.common_actions ?? []} conditions={value.conditions} conditionPath={props.conditionPath} editorScope={editorScope(props.input)} fields={props.stage?.document_fields ?? []} localTypes={props.localTypes} key={`${editorScope(props.input)}:document`} packageLabel={`${value.package.id}@${value.package.version}`} onActionsChange={(actions) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, actions } } } })} onConditionsChange={(conditions) => props.onChange({ ...props.input, draft: { ...props.input.draft, content: { type: "socket", value: { ...value, conditions } } } })} onCreateAction={(action) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "action", action))} onCreateCondition={(condition) => props.onChange((current) => appendDocumentResult(current, editorScope(props.input), "condition", condition))} />
  </section>;
}

function httpDocumentLabel(stage: HttpRuleEditorStageViewModel | undefined) {
  return stage?.package ? `${stage.package.id}@${stage.package.version}` : "Plain JSON Body（无 Schema）";
}

function DocumentEditor(props: { packageLabel: string; editorScope: string; fields: RuleDocumentSchemaFieldCapability[]; conditionPath?: RuleDocumentConditionPathCapability; localTypes: RuleLocalDocumentTypeCapability[]; commonActions: RuleCommonActionCapability[]; conditions: Condition[]; actions: UnifiedAction[]; onCreateCondition: (condition: Condition) => void; onCreateAction: (action: UnifiedAction) => void; onConditionsChange: (conditions: Condition[]) => void; onActionsChange: (actions: UnifiedAction[]) => void }) {
  const [localPath, setLocalPath] = useState("");
  const [schemaPath, setSchemaPath] = useState<string | null>(null);
  const [manualPathSelected, setManualPathSelected] = useState(false);
  const [localType, setLocalType] = useState<RuleLocalDocumentValueType | "">("");
  const [localPredicate, setLocalPredicate] = useState<RuleLocalDocumentPredicateKind | "">("");
  const [localAction, setLocalAction] = useState<RuleLocalDocumentActionKind | "">("");
  const [localIndex, setLocalIndex] = useState<number>(0);
  const [conditionValue, setConditionValue] = useState("");
  const [actionValue, setActionValue] = useState("");
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
  const documentConditions = props.conditions.filter((condition): condition is Extract<Condition, { source: "document" | "document_pattern" }> => condition.source === "document" || condition.source === "document_pattern");
  const documentActions = props.actions.filter((action) => action.source === "document" || action.source === "record_match");
  const schemaFields = documentSchemaFields(props.fields);
  const selectedSchemaField = schemaPath === null ? undefined : schemaFields.find((field) => field.name === schemaPath);
  const selectedLocalType = props.localTypes.find((capability) => capability.value_type === localType);
  const selectedPredicates = selectedSchemaField?.predicates ?? selectedLocalType?.predicates ?? [];
  const selectedActions = selectedSchemaField?.actions ?? selectedLocalType?.actions ?? [];
  const selectedLocalAction = selectedActions.find((action) => action.kind === localAction);
  const wildcard = props.conditionPath?.wildcard_token;
  const documentPath = schemaPath === null && localPath === "/" ? "" : localPath;
  const actionPathExact = !wildcard || !localPath.split("/").includes(wildcard);
  return <div className="space-y-2"><p className="text-sm font-medium">{props.packageLabel}</p><p className="text-xs text-[var(--telemetry-muted)]">Schema 字段 {schemaFields.length} 个 · 条件 {documentConditions.length} 个 · Document 动作 {documentActions.length} 个</p>
    <fieldset className="space-y-3 rounded-md border border-[var(--telemetry-line)] p-3">
      <legend className="px-1 text-xs font-medium">Document 路径条件与动作</legend>
      <div className="grid items-end gap-3 sm:grid-cols-2" data-testid="document-path-factory">
        {schemaFields.length > 0 && <Select aria-label="Document Schema 条件路径" selectedKey={schemaPath === null ? null : schemaSelectionKey(schemaPath)} onSelectionChange={(key) => {
          const field = schemaFields.find((item) => schemaSelectionKey(item.name) === String(key));
          if (!field) return;
          const path = field.name;
          setSchemaPath(path);
          setManualPathSelected(false);
          setLocalPath(path);
          setLocalType(field.type);
        }}><Label>Document Schema 条件路径</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{schemaFields.map((field) => <ListBox.Item id={schemaSelectionKey(field.name)} key={schemaSelectionKey(field.name)} textValue={schemaPathLabel(field)}>{schemaPathLabel(field)}</ListBox.Item>)}</ListBox></Select.Popover></Select>}
        <TextField><Label>手动 Document 条件路径</Label><Input aria-label="手动 Document 条件路径" className="h-10 w-full py-0" value={localPath} onChange={(event) => { setSchemaPath(null); setManualPathSelected(true); setLocalPath(event.target.value); }} /></TextField>
        <Select aria-label="Document 值类型" isDisabled={schemaPath !== null} selectedKey={localType || null} onSelectionChange={(key) => setLocalType(String(key) as typeof localType)}>
          <Label>类型</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger>
          <Select.Popover><ListBox>{props.localTypes.map((capability) => <ListBox.Item id={capability.value_type} key={capability.value_type} textValue={capability.value_type}>{capability.value_type}</ListBox.Item>)}</ListBox></Select.Popover>
        </Select>
      </div>
      <div className="grid items-end gap-3 sm:grid-cols-2" data-testid="document-condition-factory">
        <TextField><Label>匹配值</Label><Input aria-label="匹配值" className="h-10 w-full py-0" value={conditionValue} onChange={(event) => setConditionValue(event.target.value)} /></TextField>
        <Select aria-label="Document 谓词" selectedKey={localPredicate || null} onSelectionChange={(key) => setLocalPredicate(String(key) as RuleLocalDocumentPredicateKind)}><Label>谓词</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{selectedPredicates.map((predicate) => <ListBox.Item id={predicate} key={predicate} textValue={predicate}>{predicate}</ListBox.Item>)}</ListBox></Select.Popover></Select>
        <Button className="h-10 w-full sm:col-span-2" isDisabled={pending || !localType || !localPredicate || conditionValue === "" || (schemaPath === null && !manualPathSelected)} variant="outline" onPress={() => localType && localPredicate && requestCondition(documentPath, localType, localPredicate, conditionValue, "local-condition")}>添加 Document 条件</Button>
      </div>
      <div className="grid items-end gap-3 sm:grid-cols-2" data-testid="document-action-factory">
        <Select aria-label="Document 动作" selectedKey={localAction || null} onSelectionChange={(key) => setLocalAction(String(key) as RuleLocalDocumentActionKind)}><Label>动作</Label><Select.Trigger className="h-10 min-h-10 w-full"><Select.Value className="truncate" /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>{selectedActions.map((action) => <ListBox.Item id={action.kind} key={action.kind} textValue={action.kind}>{action.kind}</ListBox.Item>)}</ListBox></Select.Popover></Select>
        <TextField><Label>动作值</Label><Input aria-label="动作值" className="h-10 w-full py-0" value={actionValue} onChange={(event) => setActionValue(event.target.value)} /></TextField>
        {localAction === "insert" && <NumberField aria-label="规则本地 Insert index" minValue={0} value={localIndex} onChange={setLocalIndex}><Label>Index</Label><NumberField.Group className="h-10 min-h-10 w-full"><NumberField.Input /></NumberField.Group></NumberField>}
        <Button className="h-10 w-full sm:col-span-2" isDisabled={pending || !actionPathExact || !selectedLocalAction || (schemaPath === null && !manualPathSelected) || (localAction !== "clear" && actionValue === "")} variant="outline" onPress={() => selectedLocalAction && requestAction(documentPath, selectedLocalAction.operand_value_type ?? selectedLocalAction.target_value_type, selectedLocalAction.kind, selectedLocalAction.kind === "clear" ? null : actionValue, selectedLocalAction.kind === "insert" ? localIndex : null, "local-action")}>添加 Document 动作</Button>
      </div>
      {props.conditionPath && <p className="text-xs text-[var(--telemetry-muted)]">{props.conditionPath.wildcard_token} 仅匹配一层；展开多个节点时按 ANY 匹配。</p>}
      <p className="text-xs text-[var(--telemetry-muted)]">Wildcard 仅用于条件；Set/Clear/Insert/Append 路径必须是精确 RFC 6901。</p>
    </fieldset>
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
) {
  if (editorScope(input) !== expectedScope || input.draft.content.type !== "http") return input;
  const content = input.draft.content.value;
  const next = kind === "condition"
    ? { ...content, conditions: [...content.conditions, value as Condition] }
    : { ...content, actions: [...content.actions, wrapRuleAction(value as HttpAction)] };
  return { ...input, draft: { ...input.draft, content: { type: "http" as const, value: next } } };
}

function appendDocumentResult(input: RuleDefinitionSaveInput, expectedScope: string, kind: "condition" | "action", value: Condition | UnifiedAction) {
  if (editorScope(input) !== expectedScope) return input;
  const content = input.draft.content;
  if (content.type === "http") {
    const next = kind === "condition"
      ? { ...content.value, conditions: [...content.value.conditions, value as Condition] }
      : { ...content.value, actions: [...content.value.actions, value as UnifiedAction] };
    return { ...input, draft: { ...input.draft, content: { type: "http" as const, value: next } } };
  }
  const next = kind === "condition"
    ? { ...content.value, conditions: [...content.value.conditions, value as Condition] }
    : { ...content.value, actions: [...content.value.actions, value as UnifiedAction] };
  return { ...input, draft: { ...input.draft, content: { type: "socket" as const, value: next } } };
}

function CapabilityList({ labels }: { labels: string[] }) {
  return labels.length > 0 ? <div className="flex flex-wrap gap-1">{labels.map((label) => <span className="rounded-full bg-[var(--telemetry-soft)] px-2 py-1 text-xs" key={label}>{label}</span>)}</div> : <p className="text-xs text-[var(--telemetry-muted)]">Rust 未声明此阶段的 HTTP 能力。</p>;
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

function ruleConditions(input: RuleDefinitionSaveInput): Condition[] {
  return input.draft.content.value.conditions;
}

function ruleActions(input: RuleDefinitionSaveInput): UnifiedAction[] {
  return input.draft.content.value.actions;
}

function updateRuleConditions(input: RuleDefinitionSaveInput, conditions: Condition[]): RuleDefinitionSaveInput {
  const content = input.draft.content;
  return content.type === "http"
    ? { ...input, draft: { ...input.draft, content: { type: "http", value: { ...content.value, conditions } } } }
    : { ...input, draft: { ...input.draft, content: { type: "socket", value: { ...content.value, conditions } } } };
}

function updateRuleActions(input: RuleDefinitionSaveInput, actions: UnifiedAction[]): RuleDefinitionSaveInput {
  const content = input.draft.content;
  return content.type === "http"
    ? { ...input, draft: { ...input.draft, content: { type: "http", value: { ...content.value, actions } } } }
    : { ...input, draft: { ...input.draft, content: { type: "socket", value: { ...content.value, actions } } } };
}
