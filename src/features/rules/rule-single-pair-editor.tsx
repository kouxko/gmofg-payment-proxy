import { type ReactNode, useState } from "react";
import { Button, Input, Label, ListBox, NumberField, Select, Tabs, TextField } from "@heroui/react";
import type {
  Condition, DocumentMutation, HttpAction, HttpRuleEditorStageViewModel, RuleActionCapabilityViewModel,
  RuleActionKind, RuleCommonActionCapability, RuleDefinitionSaveInput, RuleDocumentConditionPathCapability,
  RuleNewDefinitionDraft,
  RuleLocalDocumentActionKind, RuleLocalDocumentPredicateKind,
  RuleLocalDocumentTypeCapability, RuleLocalDocumentValueType, RuleMatchFieldCapabilityViewModel,
  RuleMatchFieldKind, RuleMatchOperatorKind, SocketRuleEditorStageViewModel, UnifiedAction,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { matchFieldLabel, ruleActionKindLabel } from "./rule-definition-model";
import { documentSchemaFields, type DocumentSchemaField } from "./rule-document-schema";
import { HttpActionParametersForm } from "./rule-http-action-parameters-form";
import {
  httpActionDraft,
  httpActionParametersJson,
  newHttpActionDraft,
  type HttpActionDraft,
} from "./rule-http-action-parameters";

type Stage = HttpRuleEditorStageViewModel | SocketRuleEditorStageViewModel;
type Source = "http" | "document" | "common" | "";
const HTTP_METHOD_OPTIONS = ["GET", "POST", "PUT", "PATCH", "DELETE"] as const;

export function RuleSinglePairEditor(props: {
  actions?: ReactNode;
  creationTabs?: { title: string; cancelAction: ReactNode; basicContent: ReactNode; contentAvailable: boolean; unavailableMessage: string };
  input?: RuleDefinitionSaveInput;
  creation?: { structure: RuleNewDefinitionDraft; name: string; enabled: boolean; priority?: number; description: string };
  stage?: Stage;
  localTypes: RuleLocalDocumentTypeCapability[];
  conditionPath?: RuleDocumentConditionPathCapability;
  pending: boolean;
  onSave: (input: RuleDefinitionSaveInput) => void;
}) {
  const initialCondition = props.input?.draft.content.value.condition;
  const initialAction = props.input?.draft.content.value.action;
  const contentType = props.input?.draft.content.type ?? props.creation?.structure.content.type;
  const ruleStage = props.input?.draft.stage ?? props.creation?.structure.stage;
  const httpStage = props.stage && "http" in props.stage ? props.stage.http : null;
  const documentFields = props.stage?.document_fields ?? [];
  const commonActions = props.stage && "http" in props.stage ? props.stage.document_common_actions : props.stage?.common_actions ?? [];
  const [conditionSource, setConditionSource] = useState<Source>(() => contentType === "socket" ? "document" : conditionSourceOf(initialCondition));
  const [actionSource, setActionSource] = useState<Source>(() => actionSourceOf(initialAction));
  const [httpCondition, setHttpCondition] = useState(() => httpConditionDraft(initialCondition));
  const [conditionDocument, setConditionDocument] = useState(() => conditionDocumentDraft(initialCondition));
  const [actionDocument, setActionDocument] = useState(() => actionDocumentDraft(initialAction));
  const [httpAction, setHttpAction] = useState(() => httpActionDraft(initialAction));
  const [commonAction, setCommonAction] = useState<RuleCommonActionCapability | "">(() => initialAction?.source === "record_match" ? "record_match" : "");
  const [error, setError] = useState<string>();
  const schemaFields = documentSchemaFields(documentFields);
  const conditionSchemaFields = schemaFields.filter((field) => field.predicates.length > 0);
  const conditionLocalTypes = props.localTypes.filter((type) => type.predicates.length > 0);

  async function materialize() {
    setError(undefined);
    try {
      if (!conditionReady || !actionReady) return;
      if (conditionSource === "http" && (!httpCondition.field || !httpCondition.operator)) return;
      if (conditionSource === "document" && (!conditionDocument.type || !conditionDocument.predicate)) return;
      let condition: Condition;
      if (conditionSource === "http") {
        const field = httpCondition.field;
        const operator = httpCondition.operator;
        if (!field || !operator) return;
        if (!ruleStage) return;
        condition = await callCommand(commands.ruleDefinitionHttpConditionDraft(field, httpCondition.selector || null, operator, httpCondition.value, ruleStage));
      } else {
        const valueType = conditionDocument.type;
        const predicate = conditionDocument.predicate;
        if (!valueType || !predicate) return;
        condition = await callCommand(commands.ruleDefinitionDocumentConditionDraft(canonicalPath(conditionDocument), valueType, predicate, conditionDocument.value));
      }
      let action: UnifiedAction;
      if (actionSource === "http") {
        if (!httpAction.kind) return;
        const capability = httpStage?.actions.find((item) => item.kind === httpAction.kind);
        const raw = httpActionParametersJson(httpAction, capability);
        if (raw === undefined) return;
        if (!ruleStage) return;
        action = wrapHttpAction(await callCommand(commands.ruleDefinitionActionDraft({ kind: httpAction.kind, parameters_json: raw }, ruleStage)));
      } else if (actionSource === "common") {
        if (!commonAction) return;
        action = await callCommand(commands.ruleDefinitionDocumentCommonActionDraft(commonAction));
      } else {
        if (!actionDocument.action) return;
        const capability = selectedDocumentActions(schemaFields, props.localTypes, actionDocument).find((item) => item.kind === actionDocument.action);
        if (!capability) return;
        const valueType = capability.operand_value_type ?? capability.target_value_type;
        const raw = actionDocument.action === "clear" ? null : actionDocument.value;
        const index = actionDocument.action === "insert" ? actionDocument.index : null;
        action = await callCommand(commands.ruleDefinitionDocumentActionDraft(canonicalPath(actionDocument), valueType, actionDocument.action, raw, index));
      }
      const materialized = props.input ? withSinglePair(props.input, condition, action) : props.creation ? newSaveInput(props.creation, condition, action) : null;
      if (materialized) props.onSave(materialized);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }

  const conditionReady = conditionSource === "http"
    ? Boolean(httpCondition.field && httpCondition.operator && (selectedMatchField(httpStage?.match_fields ?? [], httpCondition.field)?.selector == null || httpCondition.selector) && (httpCondition.field !== "method" || httpCondition.value))
    : Boolean(pathReady(conditionDocument) && conditionDocument.type && conditionDocument.predicate && documentValueReady(conditionDocument.type, conditionDocument.value));
  const actionReady = actionSource === "http"
    ? httpActionParametersJson(httpAction, httpStage?.actions.find((item) => item.kind === httpAction.kind)) !== undefined
    : actionSource === "common" ? Boolean(commonAction) : documentActionReady(schemaFields, props.localTypes, actionDocument);

  const conditionForm = <ConditionForm conditionPath={props.conditionPath} document={conditionDocument} fields={conditionSchemaFields} http={httpCondition} httpFields={httpStage?.match_fields ?? []} isHttp={contentType === "http"} localTypes={conditionLocalTypes} onDocument={setConditionDocument} onHttp={setHttpCondition} onSource={setConditionSource} source={conditionSource} />;
  const actionForm = <ActionForm action={httpAction} capabilities={httpStage?.actions ?? []} commonAction={commonAction} commonActions={commonActions} document={actionDocument} fields={schemaFields} isHttp={contentType === "http"} localTypes={props.localTypes} onAction={setHttpAction} onCommonAction={setCommonAction} onDocument={setActionDocument} onSource={setActionSource} source={actionSource} />;
  const saveButton = <Button isDisabled={props.pending || !conditionReady || !actionReady} variant="primary" onPress={() => void materialize()}>保存规则</Button>;

  return <div className="space-y-4">
    {props.creationTabs && <header className="flex items-center gap-2" data-testid="rule-creation-header">
      <h2 className="text-lg font-semibold">{props.creationTabs.title}</h2>
      <div className="ml-auto flex items-center gap-2">{saveButton}{props.creationTabs.cancelAction}</div>
    </header>}
    {props.creationTabs ? <Tabs defaultSelectedKey="basic">
      <Tabs.ListContainer>
        <Tabs.List aria-label="新建规则编辑">
          <Tabs.Tab id="basic">基本信息<Tabs.Indicator /></Tabs.Tab>
          <Tabs.Tab id="conditions">匹配条件<Tabs.Indicator /></Tabs.Tab>
          <Tabs.Tab id="actions">执行动作<Tabs.Indicator /></Tabs.Tab>
        </Tabs.List>
      </Tabs.ListContainer>
      <Tabs.Panel id="basic" className="space-y-4 pt-4">{props.creationTabs.basicContent}</Tabs.Panel>
      <Tabs.Panel id="conditions" className="pt-4">
        {props.creationTabs.contentAvailable ? conditionForm : <p className="text-sm text-[var(--telemetry-muted)]">{props.creationTabs.unavailableMessage}</p>}
      </Tabs.Panel>
      <Tabs.Panel id="actions" className="pt-4">
        {props.creationTabs.contentAvailable ? actionForm : <p className="text-sm text-[var(--telemetry-muted)]">{props.creationTabs.unavailableMessage}</p>}
      </Tabs.Panel>
    </Tabs> : <>{conditionForm}{actionForm}</>}
    {error && <p className="text-sm text-red-600" role="alert">{error}</p>}
    {!props.creationTabs && <div className="flex flex-wrap items-center gap-2" data-testid="rule-editor-actions">{saveButton}{props.actions}</div>}
  </div>;
}

type HttpConditionDraft = { field: RuleMatchFieldKind | ""; operator: RuleMatchOperatorKind | ""; selector: string; value: string };
type DocumentPathDraft = { path: string; pathSet: boolean; schemaPath: string | null; type: RuleLocalDocumentValueType | "" };
type DocumentConditionDraft = DocumentPathDraft & { predicate: RuleLocalDocumentPredicateKind | ""; value: string };
type DocumentActionDraft = DocumentPathDraft & { action: RuleLocalDocumentActionKind | ""; value: string; index: number };

function ConditionForm(props: { source: Source; isHttp: boolean; http: HttpConditionDraft; httpFields: RuleMatchFieldCapabilityViewModel[]; document: DocumentConditionDraft; fields: DocumentSchemaField[]; localTypes: RuleLocalDocumentTypeCapability[]; conditionPath?: RuleDocumentConditionPathCapability; onSource: (value: Source) => void; onHttp: (value: HttpConditionDraft) => void; onDocument: (value: DocumentConditionDraft) => void }) {
  const selectedField = selectedMatchField(props.httpFields, props.http.field);
  const selectedSchema = props.fields.find((field) => field.name === props.document.schemaPath);
  const predicates = selectedSchema?.predicates ?? props.localTypes.find((item) => item.value_type === props.document.type)?.predicates ?? [];
  return <fieldset className="space-y-3 rounded-lg border border-[var(--telemetry-line)] p-3" data-testid="condition-form"><legend className="px-1 font-medium">匹配条件</legend>
    {props.isHttp && <SourceSelect label="条件来源" options={["http", "document"]} source={props.source} onSource={props.onSource} />}
    {props.source === "http" && <div className="grid items-end gap-3 sm:grid-cols-2">
      <Select aria-label="HTTP 匹配字段" selectedKey={props.http.field || null} onSelectionChange={(key) => props.onHttp({ field: String(key) as RuleMatchFieldKind, operator: "", selector: "", value: "" })}><Label>HTTP 匹配字段</Label><Select.Trigger className="h-10 min-h-10 w-full min-w-0 overflow-hidden"><Select.Value className="min-w-0 flex-1 truncate whitespace-nowrap" /><Select.Indicator className="shrink-0" /></Select.Trigger><Select.Popover><ListBox>{props.httpFields.map((field) => <ListBox.Item id={field.kind} key={field.kind} textValue={matchFieldLabel(field.kind)}>{matchFieldLabel(field.kind)}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      <Select aria-label="HTTP 匹配操作符" selectedKey={props.http.operator || null} onSelectionChange={(key) => props.onHttp({ ...props.http, operator: String(key) as RuleMatchOperatorKind })}><Label>HTTP 匹配操作符</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{(selectedField?.operators ?? []).map((operator) => <ListBox.Item id={operator} key={operator} textValue={operator}>{operator}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      {selectedField?.selector && <TextField><Label>Header selector（/name）</Label><Input aria-label="Header selector（/name）" className="h-10 w-full py-0" value={props.http.selector} onChange={(event) => props.onHttp({ ...props.http, selector: event.target.value })} /></TextField>}
      {props.http.field === "method" ? <Select aria-label="HTTP Method" selectedKey={props.http.value || null} onSelectionChange={(key) => props.onHttp({ ...props.http, value: String(key) })}><Label>HTTP Method</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{HTTP_METHOD_OPTIONS.map((method) => <ListBox.Item id={method} key={method} textValue={method}>{method}</ListBox.Item>)}</ListBox></Select.Popover></Select> : <TextField><Label>HTTP 匹配值</Label><Input aria-label="HTTP 匹配值" className="h-10 w-full py-0" value={props.http.value} onChange={(event) => props.onHttp({ ...props.http, value: event.target.value })} /></TextField>}
    </div>}
    {props.source === "document" && <DocumentConditionPathFields document={props.document} fields={props.fields} localTypes={props.localTypes} onDocument={props.onDocument} />}
    {props.source === "document" && <div className="grid items-end gap-3 sm:grid-cols-2"><Select aria-label="Document 谓词" selectedKey={props.document.predicate || null} onSelectionChange={(key) => props.onDocument({ ...props.document, predicate: String(key) as RuleLocalDocumentPredicateKind })}><Label>Document 谓词</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{predicates.map((item) => <ListBox.Item id={item} key={item} textValue={item}>{item}</ListBox.Item>)}</ListBox></Select.Popover></Select><TextField><Label>匹配值</Label><Input aria-label="匹配值" className="h-10 w-full py-0" value={props.document.value} onChange={(event) => props.onDocument({ ...props.document, value: event.target.value })} /></TextField></div>}
    {props.source === "document" && props.conditionPath && <p className="text-xs text-[var(--telemetry-muted)]">{props.conditionPath.wildcard_token} 仅匹配一层；展开多个节点时按 ANY 匹配。</p>}
  </fieldset>;
}

function ActionForm(props: { source: Source; isHttp: boolean; action: HttpActionDraft; capabilities: RuleActionCapabilityViewModel[]; commonAction: RuleCommonActionCapability | ""; commonActions: RuleCommonActionCapability[]; document: DocumentActionDraft; fields: DocumentSchemaField[]; localTypes: RuleLocalDocumentTypeCapability[]; onSource: (value: Source) => void; onAction: (value: HttpActionDraft) => void; onCommonAction: (value: RuleCommonActionCapability) => void; onDocument: (value: DocumentActionDraft) => void }) {
  const actions = selectedDocumentActions(props.fields, props.localTypes, props.document);
  const capability = props.capabilities.find((item) => item.kind === props.action.kind);
  const options: Source[] = [...(props.isHttp && props.capabilities.length ? ["http" as const] : []), "document", ...(props.commonActions.length ? ["common" as const] : [])];
  return <fieldset className="space-y-3 rounded-lg border border-[var(--telemetry-line)] p-3" data-testid="action-form"><legend className="px-1 font-medium">对应动作</legend>
    {props.source === "http" ? <>
      <div className="grid items-end gap-3 sm:grid-cols-2" data-testid="http-action-selector-row">
        <SourceSelect label="动作来源" options={options} source={props.source} onSource={props.onSource} />
        <Select aria-label="HTTP 动作类型" selectedKey={props.action.kind || null} onSelectionChange={(key) => props.onAction(newHttpActionDraft(String(key) as RuleActionKind))}><Label>HTTP 动作类型</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{props.capabilities.map(({ kind }) => <ListBox.Item id={kind} key={kind} textValue={ruleActionKindLabel(kind)}>{ruleActionKindLabel(kind)}</ListBox.Item>)}</ListBox></Select.Popover></Select>
      </div>
      {capability?.parameters_required && <div className="w-full" data-testid="http-action-parameters-row"><HttpActionParametersForm draft={props.action} onChange={props.onAction} /></div>}
    </> : <SourceSelect label="动作来源" options={options} source={props.source} onSource={props.onSource} />}
    {props.source === "document" && <><DocumentActionPathFields document={props.document} fields={props.fields} localTypes={props.localTypes} onDocument={props.onDocument} /><div className="grid items-end gap-3 sm:grid-cols-2"><Select aria-label="Document 动作" selectedKey={props.document.action || null} onSelectionChange={(key) => props.onDocument({ ...props.document, action: String(key) as RuleLocalDocumentActionKind })}><Label>Document 动作</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{actions.map(({ kind }) => <ListBox.Item id={kind} key={kind} textValue={kind}>{kind}</ListBox.Item>)}</ListBox></Select.Popover></Select><TextField><Label>动作值</Label><Input aria-label="动作值" className="h-10 w-full py-0" value={props.document.value} onChange={(event) => props.onDocument({ ...props.document, value: event.target.value })} /></TextField>{props.document.action === "insert" && <NumberField aria-label="规则本地 Insert index" minValue={0} value={props.document.index} onChange={(index) => props.onDocument({ ...props.document, index })}><Label>Index</Label><NumberField.Group className="h-10 min-h-10 w-full"><NumberField.Input /></NumberField.Group></NumberField>}</div>{props.document.path.split("/").includes("*") && <p className="text-xs text-[var(--telemetry-muted)]">* 仅展开一层；动作会应用到当前命中的全部节点。</p>}</>}
    {props.source === "common" && <Select aria-label="通用动作" selectedKey={props.commonAction || null} onSelectionChange={(key) => props.onCommonAction(String(key) as RuleCommonActionCapability)}><Label>通用动作</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{props.commonActions.map((item) => <ListBox.Item id={item} key={item} textValue={item === "record_match" ? "记录命中" : item}>{item === "record_match" ? "记录命中" : item}</ListBox.Item>)}</ListBox></Select.Popover></Select>}
  </fieldset>;
}

function SourceSelect(props: { label: string; source: Source; options: Source[]; onSource: (value: Source) => void }) {
  return <Select aria-label={props.label} selectedKey={props.source || null} onSelectionChange={(key) => props.onSource(String(key) as Source)}><Label>{props.label}</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{props.options.map((item) => { const label = item === "http" ? "HTTP" : item === "document" ? "Document" : "通用"; return <ListBox.Item id={item} key={item} textValue={label}>{label}</ListBox.Item>; })}</ListBox></Select.Popover></Select>;
}

function ClippedSelectTrigger() {
  return <Select.Trigger className="h-10 min-h-10 w-full min-w-0 overflow-hidden"><Select.Value className="min-w-0 flex-1 truncate whitespace-nowrap" /><Select.Indicator className="shrink-0" /></Select.Trigger>;
}

function DocumentConditionPathFields(props: { document: DocumentConditionDraft; fields: DocumentSchemaField[]; localTypes: RuleLocalDocumentTypeCapability[]; onDocument: (value: DocumentConditionDraft) => void }) {
  return <DocumentPathFields document={props.document} fields={props.fields} localTypes={props.localTypes} pathKind="条件" onDocument={(document) => props.onDocument({ ...props.document, ...document, predicate: "" })} />;
}

function DocumentActionPathFields(props: { document: DocumentActionDraft; fields: DocumentSchemaField[]; localTypes: RuleLocalDocumentTypeCapability[]; onDocument: (value: DocumentActionDraft) => void }) {
  return <DocumentPathFields document={props.document} fields={props.fields} localTypes={props.localTypes} pathKind="动作" onDocument={(document) => props.onDocument({ ...props.document, ...document, action: "" })} />;
}

function DocumentPathFields<T extends DocumentPathDraft>(props: { document: T; fields: DocumentSchemaField[]; localTypes: RuleLocalDocumentTypeCapability[]; pathKind: "条件" | "动作"; onDocument: (value: DocumentPathDraft) => void }) {
  const schemaAriaLabel = `Document Schema ${props.pathKind}路径`;
  const manualAriaLabel = `手动 Document ${props.pathKind}路径`;
  return <div className="grid items-end gap-3 sm:grid-cols-2">
    {props.fields.length > 0 && <Select aria-label={schemaAriaLabel} selectedKey={props.document.schemaPath == null ? null : schemaKey(props.document.schemaPath)} onSelectionChange={(key) => { const field = props.fields.find((item) => schemaKey(item.name) === String(key)); if (field) props.onDocument({ path: field.name === "" ? "/" : field.name, pathSet: true, schemaPath: field.name, type: field.type }); }}><Label>{schemaAriaLabel}</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{props.fields.map((field) => <ListBox.Item id={schemaKey(field.name)} key={schemaKey(field.name)} textValue={schemaLabel(field)}>{schemaLabel(field)}</ListBox.Item>)}</ListBox></Select.Popover></Select>}
    <TextField><Label>{manualAriaLabel}</Label><Input aria-label={manualAriaLabel} className="h-10 w-full py-0" value={props.document.path} onChange={(event) => props.onDocument({ path: event.target.value, pathSet: true, schemaPath: null, type: "" })} /></TextField>
    <Select aria-label={`Document ${props.pathKind}值类型`} isDisabled={props.document.schemaPath != null} selectedKey={props.document.type || null} onSelectionChange={(key) => props.onDocument({ ...props.document, type: String(key) as RuleLocalDocumentValueType })}><Label>类型</Label><ClippedSelectTrigger /><Select.Popover><ListBox>{props.localTypes.map((item) => <ListBox.Item id={item.value_type} key={item.value_type} textValue={item.value_type}>{item.value_type}</ListBox.Item>)}</ListBox></Select.Popover></Select>
  </div>;
}

function conditionSourceOf(value?: Condition): Source { return value?.source === "http" ? "http" : value ? "document" : ""; }
function actionSourceOf(value?: UnifiedAction): Source { return value?.source === "http" || value?.source === "terminal" ? "http" : value?.source === "document" ? "document" : value?.source === "record_match" ? "common" : ""; }
function httpConditionDraft(value?: Condition): HttpConditionDraft {
  if (value?.source !== "http") return { field: "", operator: "", selector: "", value: "" };
  const field = typeof value.field === "object" ? "header" : value.field === "Method" ? "method" : value.field === "RequestTarget" ? "request_target" : value.field === "TerminalIp" ? "terminal_ip" : "certificate_fingerprint";
  const selector = typeof value.field === "object" ? value.field.Header : "";
  const [key, raw] = Object.entries(value.operator)[0];
  const operator = ({ Equals: "equals", Contains: "contains", StartsWith: "starts_with", EndsWith: "ends_with", Wildcard: "wildcard" } as const)[key as "Equals"];
  return { field, operator, selector, value: String(raw) };
}
function conditionDocumentDraft(condition?: Condition): DocumentConditionDraft {
  const document = condition?.source === "document" || condition?.source === "document_pattern" ? condition : undefined;
  const predicate = document ? predicateDraft(document.predicate) : { type: "" as const, kind: "" as const, value: "" };
  return { path: document?.path === "" ? "/" : document?.path ?? "", pathSet: document != null, schemaPath: null, type: predicate.type, predicate: predicate.kind, value: predicate.value };
}
function actionDocumentDraft(action?: UnifiedAction): DocumentActionDraft {
  const mutation = action?.source === "document" ? action.value : undefined;
  return { path: mutation?.path === "" ? "/" : mutation?.path ?? "", pathSet: mutation != null, schemaPath: null, type: mutationType(mutation), action: mutation?.type ?? "", value: mutationValue(mutation), index: mutation?.type === "insert" ? mutation.index : 0 };
}
function predicateDraft(predicate: Extract<Condition, { source: "document" | "document_pattern" }>["predicate"]): { type: RuleLocalDocumentValueType; kind: RuleLocalDocumentPredicateKind; value: string } {
  if (predicate.type === "string") return { type: "string", kind: predicate.value.operator === "equal" ? "equals" : predicate.value.operator, value: predicate.value.value };
  if (predicate.type === "number") return { type: "number", kind: predicate.value.operator === "equal" ? "equals" : predicate.value.operator, value: String(predicate.value.value) };
  if (predicate.type === "boolean") return { type: "boolean", kind: "equals", value: String(predicate.value.equal) };
  return { type: "null", kind: "equals", value: "null" };
}
function mutationType(value?: DocumentMutation): RuleLocalDocumentValueType | "" { if (!value) return ""; const raw = value.type === "clear" ? value.value_type : value.value; if (raw === null) return "null"; if (Array.isArray(raw)) return "array"; return typeof raw as RuleLocalDocumentValueType; }
function mutationValue(value?: DocumentMutation) { if (!value || value.type === "clear") return ""; return typeof value.value === "string" ? value.value : JSON.stringify(value.value); }
function selectedMatchField(fields: RuleMatchFieldCapabilityViewModel[], kind: RuleMatchFieldKind | "") { return fields.find((item) => item.kind === kind); }
function selectedDocumentActions(fields: DocumentSchemaField[], localTypes: RuleLocalDocumentTypeCapability[], draft: DocumentActionDraft) { return fields.find((item) => item.name === draft.schemaPath)?.actions ?? localTypes.find((item) => item.value_type === draft.type)?.actions ?? []; }
function canonicalPath(draft: DocumentPathDraft) { return draft.schemaPath ?? (draft.path === "/" ? "" : draft.path); }
function pathReady(draft: DocumentPathDraft) { return draft.schemaPath != null || draft.pathSet; }
function documentValueReady(type: RuleLocalDocumentValueType | "", value: string) { return type === "string" || type === "null" || value.trim() !== ""; }
function documentActionReady(fields: DocumentSchemaField[], localTypes: RuleLocalDocumentTypeCapability[], draft: DocumentActionDraft) {
  if (!pathReady(draft) || !draft.action) return false;
  const capability = selectedDocumentActions(fields, localTypes, draft).find((item) => item.kind === draft.action);
  if (!capability) return false;
  const valueType = capability.operand_value_type ?? capability.target_value_type;
  return draft.action === "clear" || documentValueReady(valueType, draft.value);
}
function schemaKey(path: string) { return `pointer:${path}`; }
function schemaLabel(field: DocumentSchemaField) { return field.name === "" ? `${field.label} · /（根）` : field.name === "/" ? `${field.label} · /（空名称属性）` : `${field.label} · ${field.name}`; }
function wrapHttpAction(action: HttpAction): UnifiedAction { return "Terminal" in action ? { source: "terminal", value: action.Terminal! } : { source: "http", value: action }; }
function withSinglePair(input: RuleDefinitionSaveInput, condition: Condition, action: UnifiedAction): RuleDefinitionSaveInput { const content = input.draft.content; return { ...input, draft: { ...input.draft, content: content.type === "http" ? { type: "http", value: { ...content.value, condition, action } } : { type: "socket", value: { ...content.value, condition, action } } } }; }
function newSaveInput(creation: NonNullable<Parameters<typeof RuleSinglePairEditor>[0]["creation"]>, condition: Condition, action: UnifiedAction): RuleDefinitionSaveInput {
  const { structure } = creation;
  if (creation.name.trim() === "" || creation.priority == null || !Number.isSafeInteger(creation.priority) || creation.priority < 0) {
    throw new Error("规则名称和阶段内优先级尚未填写完整。");
  }
  const content = structure.content.type === "http"
    ? { type: "http" as const, value: { description: creation.description, condition, action } }
    : { type: "socket" as const, value: { package: structure.content.value.package, condition, action } };
  return { rule_id: null, expected_revision: null, draft: { name: creation.name, enabled: creation.enabled, priority: creation.priority, listener_id: structure.listener_id, stage: structure.stage, content } };
}
