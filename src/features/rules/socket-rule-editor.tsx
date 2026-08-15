import {
  Alert,
  AlertDialog,
  Button,
  Chip,
  Label,
  ListBox,
  NumberField,
  Select,
  Spinner,
  Switch,
} from "@heroui/react";
import { useState } from "react";
import type {
  DocumentAction,
  DocumentCondition,
  ProxyListener,
  SocketDirection,
  SocketRuleCapabilityCatalog,
  SocketRuleFieldCapability,
} from "@/generated/rust-types";
import {
  conditionFor,
  listenerDirections,
  setActionFor,
  type SocketRuleDraft,
} from "./socket-rule-model";
import { SocketRuleValueEditor, type SocketValueAsyncState } from "./socket-rule-value-editor";

export function SocketRuleEditor(props: {
  draft?: SocketRuleDraft;
  catalog?: SocketRuleCapabilityCatalog;
  listener?: ProxyListener;
  listeners: ProxyListener[];
  creating: boolean;
  loading: boolean;
  error?: string;
  fieldErrors: Record<string, string[]>;
  pending: boolean;
  blocked?: boolean;
  decodeEnabled: boolean;
  valueStates: Record<string, SocketValueAsyncState>;
  onValueStateChange: (key: string, state?: SocketValueAsyncState) => void;
  onResetInvalidValues: () => void;
  onListenerChange: (listenerId: string) => void;
  onDirectionChange: (direction: SocketDirection) => void;
  onChange: (draft: SocketRuleDraft) => void;
  onSave: () => void;
  onReload: () => void;
  onReloadRule: () => void;
  onDelete: () => void;
}) {
  if (props.loading) return <EditorShell><Spinner aria-label="正在读取 Socket 规则能力" /></EditorShell>;
  if (props.error) {
    return <EditorShell><Alert status="danger">
      <Alert.Indicator /><Alert.Content><Alert.Title>规则能力读取失败</Alert.Title><Alert.Description>{props.error}</Alert.Description></Alert.Content>
      <Button size="sm" variant="outline" onPress={props.onReload}>重试</Button>
    </Alert></EditorShell>;
  }
  if (!props.draft || !props.catalog || !props.listener) {
    return <EditorShell><p className="text-[var(--telemetry-muted)]">选择一条规则或新建规则进行编辑。</p></EditorShell>;
  }
  const { draft, catalog, listener } = props;
  const isLocal = listener.data_plane.kind === "socket" && listener.data_plane.settings.topology.mode === "local_responder";
  const canModify = catalog.common_actions.includes("clear_document") || catalog.fields.some((field) => field.actions.includes("set_field"));
  const valueParsing = Object.values(props.valueStates).some((state) => state.pending);
  const draftDisabled = props.pending || props.blocked || valueParsing;
  const sideEffectsDisabled = props.pending || props.blocked || valueParsing;
  const errors = unmappedFieldErrors(props.fieldErrors);
  return (
    <EditorShell>
      <h2 className="text-lg font-semibold">{props.creating ? "新建 Socket 规则" : "编辑 Socket 规则"}</h2>
      {props.creating ? (
        <>
          <CreationBinding
            direction={draft.direction}
            listener={listener}
            listeners={props.listeners}
            pending={draftDisabled}
            onDirectionChange={props.onDirectionChange}
            onListenerChange={props.onListenerChange}
          />
          <div aria-label="新规则能力绑定" className="flex flex-wrap gap-2">
            <Chip variant="soft">{draft.package.id}@{draft.package.version}</Chip>
            <Chip variant="soft">Schema v{draft.schema_version}</Chip>
          </div>
        </>
      ) : (
        <div aria-label="固定规则绑定" className="flex flex-wrap gap-2">
          <Chip variant="soft">{listener.name}</Chip>
          <Chip variant="soft">{draft.package.id}@{draft.package.version}</Chip>
          <Chip variant="soft">Schema v{draft.schema_version}</Chip>
          <Chip variant="soft">{draft.direction}</Chip>
        </div>
      )}
      <div className="flex flex-wrap gap-5">
        <Switch aria-label="启用 Socket 规则" isDisabled={draftDisabled} isSelected={draft.enabled} onChange={(enabled) => props.onChange({ ...draft, enabled })}>
          <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>启用规则</span></Switch.Content>
        </Switch>
        <NumberField isDisabled={draftDisabled} value={draft.priority} onChange={(priority) => props.onChange({ ...draft, priority })}>
          <Label>优先级</Label>
          <NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group>
        </NumberField>
      </div>
      <InlineErrors errors={fieldErrorsFor(props.fieldErrors, ["priority", "listener_id", "package", "schema_version", "direction"])} />
      {!props.decodeEnabled && isLocal && (
        <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>请求 Decode 已关闭</Alert.Title>
          <Alert.Description>字段初始未赋值，字段条件不会命中；仍可使用空条件构造静态响应。</Alert.Description>
        </Alert.Content></Alert>
      )}
      {!props.decodeEnabled && !isLocal && (
        <Alert status="warning"><Alert.Indicator /><Alert.Content>
          <Alert.Title>此方向 Decode 已关闭</Alert.Title>
          <Alert.Description>该方向不会生成可供字段条件读取的 Document。</Alert.Description>
        </Alert.Content></Alert>
      )}
      {!canModify && (
        <Alert status="accent"><Alert.Indicator /><Alert.Content>
          <Alert.Title>此方向 Encode 已关闭</Alert.Title>
          <Alert.Description>只可记录命中，线路继续发送原始 Frame。</Alert.Description>
        </Alert.Content></Alert>
      )}
      <ConditionsSection {...props} catalog={catalog} draft={draft} />
      <ActionsSection {...props} catalog={catalog} draft={draft} />
      {catalog.fields.length === 0 && (
        <Alert status="warning"><Alert.Indicator /><Alert.Content><Alert.Title>Schema 没有声明字段</Alert.Title><Alert.Description>仍可保存无条件 RecordMatch 规则。</Alert.Description></Alert.Content></Alert>
      )}
      {errors.length > 0 && (
        <Alert role="alert" status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>规则保存失败</Alert.Title><Alert.Description>{errors.join("；")}</Alert.Description></Alert.Content></Alert>
      )}
      <div className="flex gap-3">
        <Button
          isDisabled={props.pending || props.blocked || Object.keys(props.valueStates).length > 0 || draft.actions.length === 0}
          onPress={props.onSave}
          variant="primary"
        >{props.pending ? "正在保存…" : "保存 Socket 规则"}</Button>
        {!props.creating && <Button isDisabled={sideEffectsDisabled} onPress={props.onReloadRule} variant="outline">重新加载当前规则</Button>}
        {!props.creating && <DeleteRuleButton listener={listener} draft={draft} pending={sideEffectsDisabled} onDelete={props.onDelete} />}
      </div>
    </EditorShell>
  );
}

function EditorShell({ children }: { children: React.ReactNode }) {
  return <aside className="space-y-5 overflow-auto border-l border-[var(--telemetry-line)] p-5 max-[1280px]:border-l-0 max-[1280px]:border-t">{children}</aside>;
}

function CreationBinding(props: {
  listener: ProxyListener;
  listeners: ProxyListener[];
  direction: SocketDirection;
  onListenerChange: (id: string) => void;
  onDirectionChange: (direction: SocketDirection) => void;
  pending?: boolean;
}) {
  const directions = listenerDirections(props.listener);
  const isLocal = directions.length === 1;
  return <div className="grid gap-4 sm:grid-cols-2">
    <div className="grid gap-1"><Label>Listener</Label><Select aria-label="Socket Listener" isDisabled={props.pending} selectedKey={props.listener.id} onSelectionChange={(key) => props.onListenerChange(String(key))}>
      <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>
        {props.listeners.map((listener) => {
          const details = socketListenerDescription(listener);
          return <ListBox.Item id={listener.id} key={listener.id} textValue={listener.name}><span>{listener.name}</span><span className="ml-2 text-xs text-[var(--telemetry-muted)]">{details}</span></ListBox.Item>;
        })}
      </ListBox></Select.Popover>
    </Select></div>
    {isLocal ? <div><Label>方向</Label><p className="mt-2 font-mono text-sm">downstream</p></div> : <div className="grid gap-1"><Label>方向</Label><Select aria-label="Socket 方向" isDisabled={props.pending} selectedKey={props.direction} onSelectionChange={(key) => props.onDirectionChange(key as SocketDirection)}>
      <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>
        <ListBox.Item id="upstream" textValue="upstream">upstream</ListBox.Item><ListBox.Item id="downstream" textValue="downstream">downstream</ListBox.Item>
      </ListBox></Select.Popover>
    </Select></div>}
  </div>;
}

function socketListenerDescription(listener: ProxyListener) {
  if (listener.data_plane.kind !== "socket" || listener.data_plane.settings.processing?.mode !== "scripted") return "";
  const topology = listener.data_plane.settings.topology.mode === "local_responder" ? "LocalResponder" : "Relay";
  const packageRef = listener.data_plane.settings.processing.settings.package;
  return `${topology} · ${packageRef.id}@${packageRef.version}`;
}

type SectionProps = Parameters<typeof SocketRuleEditor>[0] & { draft: SocketRuleDraft; catalog: SocketRuleCapabilityCatalog };

function ConditionsSection(props: SectionProps) {
  const disabled = draftControlsDisabled(props);
  const used = new Set(props.draft.conditions.map((condition) => condition.field));
  const available = props.catalog.fields.filter((field) => !used.has(field.name));
  return <section className="space-y-3" aria-labelledby="socket-conditions-heading">
    <div className="flex items-center"><h3 id="socket-conditions-heading" className="font-semibold">条件（AND）</h3>
      <Button className="ml-auto" isDisabled={disabled || available.length === 0 || props.draft.conditions.length >= 64} size="sm" variant="outline" onPress={() => {
        if (available[0]) props.onChange({ ...props.draft, conditions: [...props.draft.conditions, conditionFor(available[0])] });
      }}>添加条件</Button>
    </div>
    {props.draft.conditions.length === 0 && <p className="text-sm text-[var(--telemetry-muted)]">空条件恒匹配。</p>}
    <InlineErrors errors={fieldErrorsFor(props.fieldErrors, ["conditions"])} />
    {props.draft.conditions.map((condition, index) => <ConditionRow key={`${condition.field}-${index}`} condition={condition} index={index} {...props} />)}
  </section>;
}

function ConditionRow(props: SectionProps & { condition: DocumentCondition; index: number }) {
  const field = props.catalog.fields.find((item) => item.name === props.condition.field);
  if (!field) return <InlineErrors errors={[`条件 ${props.index + 1} 引用了未知字段。`]} />;
  const usedElsewhere = new Set(props.draft.conditions.filter((_, index) => index !== props.index).map((item) => item.field));
  const key = `condition-${props.index}`;
  const disabled = draftControlsDisabled(props);
  return <div className="grid gap-3 rounded-lg border border-[var(--telemetry-line)] p-3 sm:grid-cols-[1fr_110px_1fr_auto]">
    <FieldSelect disabled={disabled} label="条件字段" field={field} fields={props.catalog.fields.filter((item) => !usedElsewhere.has(item.name))} onChange={(next) => { props.onResetInvalidValues(); replaceCondition(props, props.index, conditionFor(next)); }} />
    <div><Label>操作符</Label><p className="mt-2 font-mono text-sm">equals</p></div>
    <SocketRuleValueEditor disabled={valueEditorDisabled(props, key)} field={field} label="比较值" value={props.condition.value} onChange={(value) => replaceCondition(props, props.index, { ...props.condition, value })} onAsyncStateChange={(state) => props.onValueStateChange(key, state)} />
    <Button aria-label={`删除条件 ${props.index + 1}`} isDisabled={disabled} size="sm" variant="danger-soft" onPress={() => { props.onResetInvalidValues(); props.onChange({ ...props.draft, conditions: props.draft.conditions.filter((_, index) => index !== props.index) }); }}>删除</Button>
  </div>;
}

function ActionsSection(props: SectionProps) {
  const setFields = props.catalog.fields.filter((field) => field.actions.includes("set_field"));
  const disabled = draftControlsDisabled(props);
  return <section className="space-y-3" aria-labelledby="socket-actions-heading">
    <h3 id="socket-actions-heading" className="font-semibold">动作（按顺序执行）</h3>
    <InlineErrors errors={fieldErrorsFor(props.fieldErrors, ["actions"])} />
    <div className="flex flex-wrap gap-2">
      {props.catalog.common_actions.includes("record_match") && <Button isDisabled={disabled || props.draft.actions.length >= 64} size="sm" variant="outline" onPress={() => props.onChange({ ...props.draft, actions: [...props.draft.actions, { type: "record_match" }] })}>添加 RecordMatch</Button>}
      {props.catalog.common_actions.includes("clear_document") && <Button isDisabled={disabled || props.draft.actions.length >= 64} size="sm" variant="outline" onPress={() => props.onChange({ ...props.draft, actions: [...props.draft.actions, { type: "clear_document" }] })}>添加 ClearDocument</Button>}
      {setFields.length > 0 && <Button isDisabled={disabled || props.draft.actions.length >= 64} size="sm" variant="outline" onPress={() => props.onChange({ ...props.draft, actions: [...props.draft.actions, setActionFor(setFields[0])] })}>添加 SetField</Button>}
    </div>
    {props.draft.actions.map((action, index) => <ActionRow action={action} index={index} key={`${action.type}-${action.type === "set_field" ? action.field : "common"}-${index}`} {...props} />)}
  </section>;
}

function ActionRow(props: SectionProps & { action: DocumentAction; index: number }) {
  const disabled = draftControlsDisabled(props);
  const move = (offset: number) => {
    const actions = [...props.draft.actions];
    const target = props.index + offset;
    if (target < 0 || target >= actions.length) return;
    [actions[props.index], actions[target]] = [actions[target], actions[props.index]];
    props.onResetInvalidValues();
    props.onChange({ ...props.draft, actions });
  };
  const remove = () => { props.onResetInvalidValues(); props.onChange({ ...props.draft, actions: props.draft.actions.filter((_, index) => index !== props.index) }); };
  return <div className="grid gap-3 rounded-lg border border-[var(--telemetry-line)] p-3 sm:grid-cols-[1fr_auto]">
    <div>{props.action.type === "set_field" ? <SetFieldAction {...props} action={props.action} /> : <p className="font-mono text-sm">{props.action.type === "record_match" ? "RecordMatch" : "ClearDocument"}</p>}</div>
    <div className="flex gap-1">
      <Button aria-label={`动作 ${props.index + 1} 上移`} isDisabled={disabled || props.index === 0} size="sm" variant="ghost" onPress={() => move(-1)}>上移</Button>
      <Button aria-label={`动作 ${props.index + 1} 下移`} isDisabled={disabled || props.index === props.draft.actions.length - 1} size="sm" variant="ghost" onPress={() => move(1)}>下移</Button>
      <Button aria-label={`删除动作 ${props.index + 1}`} isDisabled={disabled || props.draft.actions.length === 1} size="sm" variant="danger-soft" onPress={remove}>删除</Button>
    </div>
  </div>;
}

function SetFieldAction(props: SectionProps & { action: Extract<DocumentAction, { type: "set_field" }>; index: number }) {
  const fields = props.catalog.fields.filter((field) => field.actions.includes("set_field"));
  const field = fields.find((item) => item.name === props.action.field);
  if (!field) return <InlineErrors errors={[`动作 ${props.index + 1} 引用了不可修改字段。`]} />;
  const key = `action-${props.index}`;
  return <div className="grid gap-3 sm:grid-cols-2">
    <FieldSelect disabled={draftControlsDisabled(props)} label="设置字段" field={field} fields={fields} onChange={(next) => { props.onResetInvalidValues(); replaceAction(props, props.index, setActionFor(next)); }} />
    <SocketRuleValueEditor disabled={valueEditorDisabled(props, key)} field={field} label="设置值" value={props.action.value} onChange={(value) => replaceAction(props, props.index, { ...props.action, value })} onAsyncStateChange={(state) => props.onValueStateChange(key, state)} />
  </div>;
}

function FieldSelect({ disabled, field, fields, label, onChange }: { disabled: boolean; field: SocketRuleFieldCapability; fields: SocketRuleFieldCapability[]; label: string; onChange: (field: SocketRuleFieldCapability) => void }) {
  return <div className="grid gap-1"><Label>{label}</Label><Select aria-label={label} isDisabled={disabled} selectedKey={field.name} onSelectionChange={(key) => { const next = fields.find((item) => item.name === key); if (next) onChange(next); }}>
    <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger><Select.Popover><ListBox>
      {fields.map((item) => <ListBox.Item id={item.name} key={item.name} textValue={`${item.label} ${item.name} ${item.type}`}><span>{item.label}</span> <code>{item.name}</code> <span>{item.type}</span></ListBox.Item>)}
    </ListBox></Select.Popover>
  </Select><p className="text-xs text-[var(--telemetry-muted)]"><span>{field.label}</span> · <code>{field.name}</code> · <span>{field.type}</span></p></div>;
}

function fieldErrorsFor(fieldErrors: Record<string, string[]>, prefixes: string[]) {
  return Object.entries(fieldErrors)
    .filter(([field]) => prefixes.some((prefix) => field === prefix || field.startsWith(`${prefix}.`) || field.startsWith(`${prefix}[`)))
    .flatMap(([, messages]) => messages);
}

function unmappedFieldErrors(fieldErrors: Record<string, string[]>) {
  const mapped = ["priority", "listener_id", "package", "schema_version", "direction", "conditions", "actions"];
  return Object.entries(fieldErrors)
    .filter(([field]) => !mapped.some((prefix) => field === prefix || field.startsWith(`${prefix}.`) || field.startsWith(`${prefix}[`)))
    .flatMap(([, messages]) => messages);
}

function InlineErrors({ errors }: { errors: string[] }) {
  return errors.length > 0 ? <p className="text-sm text-[var(--telemetry-danger)]" role="alert">{errors.join("；")}</p> : null;
}

function replaceCondition(props: SectionProps, index: number, condition: DocumentCondition) {
  props.onChange({ ...props.draft, conditions: props.draft.conditions.map((item, itemIndex) => itemIndex === index ? condition : item) });
}
function replaceAction(props: SectionProps, index: number, action: DocumentAction) {
  props.onChange({ ...props.draft, actions: props.draft.actions.map((item, itemIndex) => itemIndex === index ? action : item) });
}

function draftControlsDisabled(props: SectionProps) {
  return props.pending || props.blocked || Object.values(props.valueStates).some((state) => state.pending);
}

function valueEditorDisabled(props: SectionProps, key: string) {
  const parsing = Object.values(props.valueStates).some((state) => state.pending);
  return Boolean(props.pending || props.blocked || (parsing && !props.valueStates[key]?.pending));
}

function DeleteRuleButton({ listener, draft, pending, onDelete }: { listener: ProxyListener; draft: SocketRuleDraft; pending: boolean; onDelete: () => void }) {
  const [open, setOpen] = useState(false);
  return <AlertDialog isOpen={open} onOpenChange={(next) => { if (!pending) setOpen(next); }}><Button isDisabled={pending} onPress={() => setOpen(true)} variant="danger-soft">删除规则</Button><AlertDialog.Backdrop><AlertDialog.Container><AlertDialog.Dialog>
    <AlertDialog.Header><AlertDialog.Heading>删除此 Socket 规则？</AlertDialog.Heading></AlertDialog.Header>
    <AlertDialog.Body>{listener.name} · {draft.direction} · {draft.rule_id}</AlertDialog.Body>
    <AlertDialog.Footer><Button slot="close" isDisabled={pending} variant="outline">取消</Button><Button isDisabled={pending} onPress={onDelete} variant="danger">确认删除</Button></AlertDialog.Footer>
  </AlertDialog.Dialog></AlertDialog.Container></AlertDialog.Backdrop></AlertDialog>;
}
