"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  Button,
  FieldError,
  Input,
  Label,
  ListBox,
  NumberField,
  Select,
  TextArea,
  TextField,
  toast,
} from "@heroui/react";
import { Plus, TrashBin } from "@gravity-ui/icons";
import type {
  RuleAction,
  RuleActionKind,
  RuleCondition,
  RuleConditionKind,
  RuleDraft,
  RuleMatchField,
  RuleMatchFieldKind,
  RuleMatchOperator,
  RuleMatchOperatorKind,
  RuleTerminalAction,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";

export type ConditionKind = RuleConditionKind;
export type ActionKind = RuleActionKind;
export type RuleDraftChange =
  | RuleDraft
  | ((current: RuleDraft) => RuleDraft);

type AsyncEditorState = { pending: boolean; invalid: boolean };
type AsyncStateChange = (key: string, state?: AsyncEditorState) => void;
type ConditionUpdate = (current: RuleCondition) => RuleCondition;
type ActionUpdate = (current: RuleAction) => RuleAction;
type TerminalActionUpdate = (
  current: RuleTerminalAction,
) => RuleTerminalAction;

function useAsyncRequestSlots(
  prefix: string,
  onAsyncStateChange: AsyncStateChange,
) {
  const generations = useRef(new Map<string, number>());
  const activeKeys = useRef(new Set<string>());

  useEffect(() => {
    const currentGenerations = generations.current;
    const currentKeys = activeKeys.current;
    return () => {
      currentKeys.forEach((key) => {
        currentGenerations.set(key, (currentGenerations.get(key) ?? 0) + 1);
        onAsyncStateChange(key, undefined);
      });
      currentKeys.clear();
    };
  }, [onAsyncStateChange, prefix]);

  return useCallback(
    async <T,>(
      slot: string,
      request: () => Promise<T>,
      apply: (value: T) => void,
    ) => {
      const key = `${prefix}:${slot}`;
      const generation = (generations.current.get(key) ?? 0) + 1;
      generations.current.set(key, generation);
      activeKeys.current.add(key);
      onAsyncStateChange(key, { pending: true, invalid: false });
      try {
        const value = await request();
        if (generations.current.get(key) !== generation) return;
        apply(value);
        activeKeys.current.delete(key);
        onAsyncStateChange(key, undefined);
      } catch (reason) {
        if (generations.current.get(key) !== generation) return;
        onAsyncStateChange(key, { pending: false, invalid: true });
        toast(errorMessage(reason), { variant: "danger" });
      }
    },
    [onAsyncStateChange, prefix],
  );
}

function errorText(
  fieldErrors: Record<string, string[]>,
  prefix: string,
): string | undefined {
  const messages = Object.entries(fieldErrors)
    .filter(([field]) => field === prefix || field.startsWith(`${prefix}.`))
    .flatMap(([, values]) => values);
  return messages.length > 0 ? [...new Set(messages)].join("；") : undefined;
}

export function requestConditionDraft(
  kind: ConditionKind,
): Promise<RuleCondition> {
  return callCommand(commands.ruleConditionDraft(kind));
}

export function requestActionDraft(kind: ActionKind): Promise<RuleAction> {
  return callCommand(commands.ruleActionDraft(kind));
}

export function requestMatchFieldDraft(
  kind: RuleMatchFieldKind,
): Promise<RuleMatchField> {
  return callCommand(commands.ruleMatchFieldDraft(kind));
}

export function requestMatchOperatorDraft(
  kind: RuleMatchOperatorKind,
): Promise<RuleMatchOperator> {
  return callCommand(commands.ruleMatchOperatorDraft(kind));
}

export function parseRuleByteInput(raw: string) {
  return callCommand(commands.ruleParseByteInput(raw));
}

export function parseRuleHeaderInput(raw: string) {
  return callCommand(commands.ruleParseHeaderInput(raw));
}

export function actionKind(action: RuleAction): ActionKind {
  return action.type === "terminal" ? action.action.type : action.type;
}

const fieldLabels: Record<RuleMatchField["type"], string> = {
  terminal_ip: "终端 IP",
  certificate_fingerprint: "证书指纹",
  path_or_request_type: "路径 / 请求类型",
  json_path: "JSON Path",
};

const actionLabels: Record<ActionKind, string> = {
  set_json_field: "设置 JSON 字段",
  replace_body_text: "替换 Body 文本",
  set_header: "设置 Header",
  delay: "延迟",
  pause: "暂停并进入断点",
  custom_http_status: "自定义 HTTP 状态码",
  reject_tls_handshake: "拒绝 TLS 握手",
  disconnect_before_upstream: "连接上游前断开",
  upstream_connect_timeout: "上游连接超时",
  upstream_write_timeout: "上游写入超时",
  upstream_read_timeout: "上游读取超时",
  drop_upstream_response: "丢弃上游响应",
  mock_response: "Mock 响应",
  invalid_json: "非法 JSON 响应",
  incorrect_content_length: "错误 Content-Length",
  truncate_response: "截断响应",
};

const actionKinds = Object.keys(actionLabels) as ActionKind[];

function NumericInput({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <NumberField value={value} onChange={onChange}>
      <Label>{label}</Label>
      <NumberField.Group className="w-full">
        <NumberField.DecrementButton />
        <NumberField.Input />
        <NumberField.IncrementButton />
      </NumberField.Group>
    </NumberField>
  );
}

function ConditionEditor({
  condition,
  onChange,
  asyncStateKey,
  onAsyncStateChange,
}: {
  condition: RuleCondition;
  onChange: (update: ConditionUpdate) => void;
  asyncStateKey: string;
  onAsyncStateChange: AsyncStateChange;
}) {
  const runAsync = useAsyncRequestSlots(
    asyncStateKey,
    onAsyncStateChange,
  );

  if (condition.type === "nth_hit") {
    return (
      <NumericInput
        label="第 N 次命中"
        value={condition.count}
        onChange={(count) =>
          onChange((current) =>
            current.type === "nth_hit" ? { ...current, count } : current,
          )
        }
      />
    );
  }

  const field = condition.field;
  const operator = condition.operator;
  return (
    <div className="grid gap-3">
      <div className="grid gap-1">
        <Label>匹配字段</Label>
        <Select
          aria-label="匹配字段"
          selectedKey={field.type}
          onSelectionChange={(type) => {
            void runAsync(
              "field",
              () => requestMatchFieldDraft(type as RuleMatchFieldKind),
              (next) =>
                onChange((current) =>
                  current.type === "field"
                    ? { ...current, field: next }
                    : current,
                ),
            );
          }}
        >
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              {Object.entries(fieldLabels).map(([value, label]) => (
                <ListBox.Item key={value} id={value} textValue={label}>
                  {label}
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
      </div>
      {field.type === "json_path" && (
        <Input
          aria-label="JSON Path"
          value={field.path}
          onChange={(event) =>
            onChange((current) =>
              current.type === "field" &&
              current.field.type === "json_path"
                ? {
                    ...current,
                    field: { ...current.field, path: event.target.value },
                  }
                : current,
            )
          }
        />
      )}
      <div className="grid gap-1">
        <Label>操作符</Label>
        <Select
          aria-label="操作符"
          selectedKey={operator.type}
          onSelectionChange={(type) => {
            void runAsync(
              "operator",
              () => requestMatchOperatorDraft(type as RuleMatchOperatorKind),
              (next) =>
                onChange((current) =>
                  current.type === "field"
                    ? { ...current, operator: next }
                    : current,
                ),
            );
          }}
        >
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              <ListBox.Item id="equals">等于</ListBox.Item>
              <ListBox.Item id="contains">包含</ListBox.Item>
              <ListBox.Item id="regex">正则</ListBox.Item>
            </ListBox>
          </Select.Popover>
        </Select>
      </div>
      <Input
        aria-label={operator.type === "regex" ? "正则表达式" : "匹配值"}
        value={operator.type === "regex" ? operator.pattern : operator.value}
        onChange={(event) =>
          onChange((current) => {
            if (current.type !== "field") return current;
            return {
              ...current,
              operator:
                current.operator.type === "regex"
                  ? {
                      ...current.operator,
                      pattern: event.target.value,
                    }
                  : {
                      ...current.operator,
                      value: event.target.value,
                    },
            };
          })
        }
      />
    </div>
  );
}

function byteText(bytes: number[]) {
  return bytes.join(", ");
}

function TerminalActionFields({
  action,
  onChange,
  onAsyncStateChange,
}: {
  action: RuleTerminalAction;
  onChange: (update: TerminalActionUpdate) => void;
  onAsyncStateChange: (
    field: "bytes" | "headers",
    state?: { pending: boolean; invalid: boolean },
  ) => void;
}) {
  const currentBytes =
    action.type === "mock_response" || action.type === "invalid_json"
      ? action.shift_jis_body
      : [];
  const normalizedBytes = byteText(currentBytes);
  const [rawBytes, setRawBytes] = useState(normalizedBytes);
  const [byteError, setByteError] = useState<string>();
  const initialHeaders =
    action.type === "mock_response"
      ? action.headers.map(([name, value]) => `${name}: ${value}`).join("\n")
      : "";
  const [rawHeaders, setRawHeaders] = useState(initialHeaders);
  const [headerError, setHeaderError] = useState<string>();
  const byteGeneration = useRef(0);
  const headerGeneration = useRef(0);
  const asyncStateChangeRef = useRef(onAsyncStateChange);
  useEffect(() => {
    asyncStateChangeRef.current = onAsyncStateChange;
  }, [onAsyncStateChange]);
  useEffect(
    () => () => {
      byteGeneration.current += 1;
      headerGeneration.current += 1;
      asyncStateChangeRef.current("bytes", undefined);
      asyncStateChangeRef.current("headers", undefined);
    },
    [],
  );

  async function commitRawBytes(raw: string) {
    if (action.type !== "mock_response" && action.type !== "invalid_json") {
      return;
    }
    const generation = byteGeneration.current + 1;
    byteGeneration.current = generation;
    onAsyncStateChange("bytes", { pending: true, invalid: false });
    try {
      const parsed = await parseRuleByteInput(raw);
      if (generation !== byteGeneration.current) return;
      setRawBytes(parsed.normalized);
      setByteError(undefined);
      onChange((current) =>
        current.type === "mock_response" || current.type === "invalid_json"
          ? { ...current, shift_jis_body: parsed.bytes }
          : current,
      );
      onAsyncStateChange("bytes", undefined);
    } catch (reason) {
      if (generation !== byteGeneration.current) return;
      setByteError(errorMessage(reason));
      onAsyncStateChange("bytes", { pending: false, invalid: true });
    }
  }

  async function commitRawHeaders(raw: string) {
    if (action.type !== "mock_response") return;
    const generation = headerGeneration.current + 1;
    headerGeneration.current = generation;
    onAsyncStateChange("headers", { pending: true, invalid: false });
    try {
      const parsed = await parseRuleHeaderInput(raw);
      if (generation !== headerGeneration.current) return;
      setRawHeaders(parsed.normalized);
      setHeaderError(undefined);
      onChange((current) =>
        current.type === "mock_response"
          ? { ...current, headers: parsed.headers }
          : current,
      );
      onAsyncStateChange("headers", undefined);
    } catch (reason) {
      if (generation !== headerGeneration.current) return;
      setHeaderError(errorMessage(reason));
      onAsyncStateChange("headers", { pending: false, invalid: true });
    }
  }

  switch (action.type) {
    case "upstream_connect_timeout":
    case "upstream_write_timeout":
    case "upstream_read_timeout":
      return (
        <NumericInput
          label="超时（毫秒）"
          value={action.milliseconds}
          onChange={(milliseconds) =>
            onChange((current) =>
              current.type === action.type
                ? { ...current, milliseconds }
                : current,
            )
          }
        />
      );
    case "drop_upstream_response":
      return (
        <div className="grid gap-1">
          <Label>丢弃模式</Label>
          <Select
            aria-label="丢弃模式"
            selectedKey={action.mode}
            onSelectionChange={(mode) =>
              onChange((current) =>
                current.type === "drop_upstream_response"
                  ? {
                      ...current,
                      mode: mode as typeof action.mode,
                    }
                  : current,
              )
            }
          >
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                <ListBox.Item id="read_complete_response">
                  读完响应后关闭
                </ListBox.Item>
                <ListBox.Item id="close_after_request_write">
                  写完请求后关闭
                </ListBox.Item>
              </ListBox>
            </Select.Popover>
          </Select>
        </div>
      );
    case "mock_response":
      return (
        <div className="grid gap-3">
          <NumericInput
            label="HTTP 状态码"
            value={action.status}
            onChange={(status) =>
              onChange((current) =>
                current.type === "mock_response"
                  ? { ...current, status }
                  : current,
              )
            }
          />
          <TextField isInvalid={headerError != null}>
            <Label>响应 Header（每行 name: value）</Label>
            <TextArea
              aria-label="响应 Header（每行 name: value）"
              value={rawHeaders}
              onChange={(event) => {
                const raw = event.target.value;
                setRawHeaders(raw);
                setHeaderError(undefined);
                void commitRawHeaders(raw);
              }}
            />
            {headerError && <FieldError>{headerError}</FieldError>}
          </TextField>
          <TextField isInvalid={byteError != null}>
            <Label>Shift-JIS Body 字节</Label>
            <Input
              value={rawBytes}
              onChange={(event) => {
                const raw = event.target.value;
                setRawBytes(raw);
                setByteError(undefined);
                void commitRawBytes(raw);
              }}
            />
            {byteError && <FieldError>{byteError}</FieldError>}
          </TextField>
        </div>
      );
    case "invalid_json":
      return (
        <TextField isInvalid={byteError != null}>
          <Label>Shift-JIS Body 字节</Label>
          <Input
            value={rawBytes}
            onChange={(event) => {
              const raw = event.target.value;
              setRawBytes(raw);
              setByteError(undefined);
              void commitRawBytes(raw);
            }}
          />
          {byteError && <FieldError>{byteError}</FieldError>}
        </TextField>
      );
    case "incorrect_content_length":
      return (
        <NumericInput
          label="长度差值"
          value={action.delta}
          onChange={(delta) =>
            onChange((current) =>
              current.type === "incorrect_content_length"
                ? { ...current, delta }
                : current,
            )
          }
        />
      );
    case "truncate_response":
      return (
        <NumericInput
          label="截断字节数"
          value={action.bytes}
          onChange={(bytes) =>
            onChange((current) =>
              current.type === "truncate_response"
                ? { ...current, bytes }
                : current,
            )
          }
        />
      );
    case "reject_tls_handshake":
    case "disconnect_before_upstream":
      return null;
  }
}

function ActionEditor({
  action,
  onChange,
  asyncStateKey,
  onAsyncStateChange,
}: {
  action: RuleAction;
  onChange: (update: ActionUpdate) => void;
  asyncStateKey: string;
  onAsyncStateChange: AsyncStateChange;
}) {
  const runAsync = useAsyncRequestSlots(
    asyncStateKey,
    onAsyncStateChange,
  );
  const fields = (() => {
    switch (action.type) {
      case "set_json_field":
        return (
          <>
            <Input
              aria-label="JSON Path"
              value={action.path}
              onChange={(event) =>
                onChange((current) =>
                  current.type === "set_json_field"
                    ? { ...current, path: event.target.value }
                    : current,
                )
              }
            />
            <TextArea
              aria-label="JSON 值"
              value={action.value_json}
              onChange={(event) =>
                onChange((current) =>
                  current.type === "set_json_field"
                    ? { ...current, value_json: event.target.value }
                    : current,
                )
              }
            />
          </>
        );
      case "replace_body_text":
        return (
          <TextArea
            aria-label="Body 文本"
            value={action.text}
            onChange={(event) =>
              onChange((current) =>
                current.type === "replace_body_text"
                  ? { ...current, text: event.target.value }
                  : current,
              )
            }
          />
        );
      case "set_header":
        return (
          <>
            <Input
              aria-label="Header 名称"
              value={action.name}
              onChange={(event) =>
                onChange((current) =>
                  current.type === "set_header"
                    ? { ...current, name: event.target.value }
                    : current,
                )
              }
            />
            <Input
              aria-label="Header 值"
              value={action.value}
              onChange={(event) =>
                onChange((current) =>
                  current.type === "set_header"
                    ? { ...current, value: event.target.value }
                    : current,
                )
              }
            />
          </>
        );
      case "delay":
        return (
          <NumericInput
            label="延迟（毫秒）"
            value={action.milliseconds}
            onChange={(milliseconds) =>
              onChange((current) =>
                current.type === "delay"
                  ? { ...current, milliseconds }
                  : current,
              )
            }
          />
        );
      case "custom_http_status":
        return (
          <NumericInput
            label="HTTP 状态码"
            value={action.status}
            onChange={(status) =>
              onChange((current) =>
                current.type === "custom_http_status"
                  ? { ...current, status }
                  : current,
              )
            }
          />
        );
      case "terminal":
        return (
          <TerminalActionFields
            action={action.action}
            onChange={(update) =>
              onChange((current) =>
                current.type === "terminal"
                  ? {
                      ...current,
                      action: update(current.action),
                    }
                  : current,
              )
            }
            onAsyncStateChange={(field, state) =>
              onAsyncStateChange(`${asyncStateKey}:${field}`, state)
            }
          />
        );
      case "pause":
        return (
          <p className="text-sm text-[var(--telemetry-muted)]">
            命中后暂停消息，等待断点工作台处理。
          </p>
        );
    }
  })();

  return (
    <div className="grid gap-3">
      <div className="grid gap-1">
        <Label>动作类型</Label>
        <Select
          aria-label="动作类型"
          selectedKey={actionKind(action)}
          onSelectionChange={(kind) => {
            void runAsync(
              "kind",
              () => requestActionDraft(kind as ActionKind),
              (next) => onChange(() => next),
            );
          }}
        >
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              {actionKinds.map((kind) => (
                <ListBox.Item
                  key={kind}
                  id={kind}
                  textValue={actionLabels[kind]}
                >
                  {actionLabels[kind]}
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
      </div>
      {fields}
    </div>
  );
}

function ConditionRow({
  condition,
  index,
  fieldErrors,
  asyncStateKey,
  onChange,
  onDelete,
  onAsyncStateChange,
}: {
  condition: RuleCondition;
  index: number;
  fieldErrors: Record<string, string[]>;
  asyncStateKey: string;
  onChange: (update: ConditionUpdate) => void;
  onDelete: () => void;
  onAsyncStateChange: AsyncStateChange;
}) {
  const runAsync = useAsyncRequestSlots(
    asyncStateKey,
    onAsyncStateChange,
  );

  return (
    <div className="rounded-xl border border-[var(--telemetry-line)] p-3">
      <div className="mb-3 flex items-center gap-3">
        <div className="grid flex-1 gap-1">
          <Label>{`条件 ${index + 1}`}</Label>
          <Select
            aria-label={`条件 ${index + 1} 类型`}
            selectedKey={condition.type}
            onSelectionChange={(kind) => {
              void runAsync(
                "kind",
                () => requestConditionDraft(kind as ConditionKind),
                (next) => onChange(() => next),
              );
            }}
          >
            <Select.Trigger>
              <Select.Value />
              <Select.Indicator />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                <ListBox.Item id="field" textValue="字段匹配">
                  字段匹配
                </ListBox.Item>
                <ListBox.Item id="nth_hit" textValue="第 N 次命中">
                  第 N 次命中
                </ListBox.Item>
              </ListBox>
            </Select.Popover>
          </Select>
        </div>
        <Button
          isIconOnly
          variant="danger-soft"
          aria-label={`删除条件 ${index + 1}`}
          onPress={onDelete}
        >
          <TrashBin className="size-4" />
        </Button>
      </div>
      <ConditionEditor
        condition={condition}
        onChange={onChange}
        asyncStateKey={asyncStateKey}
        onAsyncStateChange={onAsyncStateChange}
      />
      {errorText(fieldErrors, `conditions.${index}`) && (
        <FieldError className="mt-2">
          {errorText(fieldErrors, `conditions.${index}`)}
        </FieldError>
      )}
    </div>
  );
}

export function ConditionsEditor({
  draft,
  fieldErrors,
  onChange,
  onAsyncStateChange,
}: {
  draft: RuleDraft;
  fieldErrors: Record<string, string[]>;
  onChange: (change: RuleDraftChange) => void;
  onAsyncStateChange: AsyncStateChange;
}) {
  const editorKey = draft.rule_id ?? "new";
  const runAsync = useAsyncRequestSlots(
    `${editorKey}:conditions-list`,
    onAsyncStateChange,
  );
  const update = (index: number, updateCondition: ConditionUpdate) =>
    onChange((current) => ({
      ...current,
      conditions: current.conditions.map((item, itemIndex) =>
        itemIndex === index ? updateCondition(item) : item,
      ),
    }));

  return (
    <div className="grid gap-3">
      {fieldErrors.conditions && (
        <FieldError>{fieldErrors.conditions.join("；")}</FieldError>
      )}
      {draft.conditions.map((condition, index) => (
        <ConditionRow
          key={`${editorKey}:${draft.conditions.length}:${index}:${condition.type}`}
          condition={condition}
          index={index}
          fieldErrors={fieldErrors}
          asyncStateKey={`${editorKey}:condition:${index}`}
          onChange={(next) => update(index, next)}
          onDelete={() =>
            onChange((current) => ({
              ...current,
              conditions: current.conditions.filter(
                (_, itemIndex) => itemIndex !== index,
              ),
            }))
          }
          onAsyncStateChange={onAsyncStateChange}
        />
      ))}
      <Button
        variant="outline"
        onPress={() => {
          void runAsync("add", () => requestConditionDraft("field"), (condition) =>
            onChange((current) => ({
              ...current,
              conditions: [...current.conditions, condition],
            })),
          );
        }}
      >
        <Plus className="size-4" />
        添加条件
      </Button>
    </div>
  );
}

export function ActionsEditor({
  draft,
  fieldErrors,
  onChange,
  onAsyncStateChange,
}: {
  draft: RuleDraft;
  fieldErrors: Record<string, string[]>;
  onChange: (change: RuleDraftChange) => void;
  onAsyncStateChange: AsyncStateChange;
}) {
  const editorKey = draft.rule_id ?? "new";
  const runAsync = useAsyncRequestSlots(
    `${editorKey}:actions`,
    onAsyncStateChange,
  );
  const update = (index: number, updateAction: ActionUpdate) =>
    onChange((current) => ({
      ...current,
      actions: current.actions.map((item, itemIndex) =>
        itemIndex === index ? updateAction(item) : item,
      ),
    }));

  return (
    <div className="grid gap-3">
      {fieldErrors.actions && (
        <FieldError>{fieldErrors.actions.join("；")}</FieldError>
      )}
      {draft.actions.map((action, index) => (
        <div
          key={`${editorKey}:${draft.actions.length}:${index}:${actionKind(action)}`}
          className="rounded-xl border border-[var(--telemetry-line)] p-3"
        >
          <div className="mb-3 flex items-center justify-between">
            <span className="text-sm font-semibold">动作 {index + 1}</span>
            <Button
              isIconOnly
              variant="danger-soft"
              aria-label={`删除动作 ${index + 1}`}
              onPress={() =>
                onChange((current) => ({
                  ...current,
                  actions: current.actions.filter(
                    (_, itemIndex) => itemIndex !== index,
                  ),
                }))
              }
            >
              <TrashBin className="size-4" />
            </Button>
          </div>
          <ActionEditor
            action={action}
            onChange={(next) => update(index, next)}
            asyncStateKey={`${editorKey}:${index}`}
            onAsyncStateChange={onAsyncStateChange}
          />
          {errorText(fieldErrors, `actions.${index}`) && (
            <FieldError className="mt-2">
              {errorText(fieldErrors, `actions.${index}`)}
            </FieldError>
          )}
        </div>
      ))}
      <Button
        variant="outline"
        onPress={() => {
          void runAsync("add", () => requestActionDraft("delay"), (action) =>
            onChange((current) => ({
              ...current,
              actions: [...current.actions, action],
            })),
          );
        }}
      >
        <Plus className="size-4" />
        添加动作
      </Button>
    </div>
  );
}
