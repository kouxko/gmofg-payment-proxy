import {
  Button,
  Input,
  Label,
  ListBox,
  Select,
} from "@heroui/react";
import { Plus, TrashBin } from "@gravity-ui/icons";
import type {
  RuleCondition,
  RuleDraft,
  RuleMatchFieldKind,
  RuleMatchOperatorKind,
  RuleStageCapabilityViewModel,
  MessageStage,
} from "@/generated/rust-types";
import { NumericInput } from "./rule-editor-controls";
import {
  errorText,
  fieldLabels,
  requestConditionDraft,
  requestMatchFieldDraft,
  requestMatchOperatorDraft,
  useAsyncRequestSlots,
  type AsyncStateChange,
  type ConditionKind,
  type ConditionUpdate,
  type RuleDraftChange,
} from "./rule-editor-model";

function ConditionFields({
  condition,
  onChange,
  asyncStateKey,
  onAsyncStateChange,
  matchFieldKinds,
  stage,
}: {
  condition: RuleCondition;
  onChange: (update: ConditionUpdate) => void;
  asyncStateKey: string;
  onAsyncStateChange: AsyncStateChange;
  matchFieldKinds: RuleMatchFieldKind[];
  stage: MessageStage;
}) {
  const runAsync = useAsyncRequestSlots(asyncStateKey, onAsyncStateChange);
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

  const { field, operator } = condition;
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
              () =>
                requestMatchFieldDraft(type as RuleMatchFieldKind, stage),
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
              {matchFieldKinds.map((value) => (
                <ListBox.Item
                  key={value}
                  id={value}
                  textValue={fieldLabels[value]}
                >
                  {fieldLabels[value]}
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
              current.type === "field" && current.field.type === "json_path"
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
          onChange((current) =>
            current.type !== "field"
              ? current
              : {
                  ...current,
                  operator:
                    current.operator.type === "regex"
                      ? { ...current.operator, pattern: event.target.value }
                      : { ...current.operator, value: event.target.value },
                },
          )
        }
      />
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
  matchFieldKinds,
  stage,
}: {
  condition: RuleCondition;
  index: number;
  fieldErrors: Record<string, string[]>;
  asyncStateKey: string;
  onChange: (update: ConditionUpdate) => void;
  onDelete: () => void;
  onAsyncStateChange: AsyncStateChange;
  matchFieldKinds: RuleMatchFieldKind[];
  stage: MessageStage;
}) {
  const runAsync = useAsyncRequestSlots(asyncStateKey, onAsyncStateChange);
  const rowError = errorText(fieldErrors, `conditions.${index}`);
  return (
    <div className="rounded-xl border border-[var(--telemetry-line)] p-3">
      <div className="mb-3 flex min-h-8 items-center justify-between gap-3">
        <span className="text-sm font-semibold">条件 {index + 1}</span>
        <Button
          isIconOnly
          variant="danger-soft"
          aria-label={`删除条件 ${index + 1}`}
          onPress={onDelete}
        >
          <TrashBin className="size-4" />
        </Button>
      </div>
      <div className="mb-3 grid gap-1">
        <Label>条件类型</Label>
        <Select
          aria-label={`条件 ${index + 1} 类型`}
          selectedKey={condition.type}
          onSelectionChange={(kind) => {
            void runAsync(
              "kind",
              () => requestConditionDraft(kind as ConditionKind, stage),
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
      <ConditionFields
        condition={condition}
        onChange={onChange}
        asyncStateKey={asyncStateKey}
        onAsyncStateChange={onAsyncStateChange}
        matchFieldKinds={matchFieldKinds}
        stage={stage}
      />
      {condition.type === "field" &&
        !matchFieldKinds.includes(condition.field.type) && (
          <p
            className="mt-2 text-sm text-[var(--telemetry-danger)]"
            role="alert"
          >
            当前匹配字段不支持所选阶段，请改为下拉框中的可用字段。
          </p>
        )}
      {rowError && (
        <p
          className="mt-2 text-sm text-[var(--telemetry-danger)]"
          role="alert"
        >
          {rowError}
        </p>
      )}
    </div>
  );
}

export function ConditionsEditor({
  draft,
  fieldErrors,
  onChange,
  onAsyncStateChange,
  capability,
}: {
  draft: RuleDraft;
  fieldErrors: Record<string, string[]>;
  onChange: (change: RuleDraftChange) => void;
  onAsyncStateChange: AsyncStateChange;
  capability?: RuleStageCapabilityViewModel;
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
        <p className="text-sm text-[var(--telemetry-danger)]" role="alert">
          {fieldErrors.conditions.join("；")}
        </p>
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
          matchFieldKinds={capability?.match_field_kinds ?? []}
          stage={draft.stage!}
        />
      ))}
      <Button
        variant="outline"
        isDisabled={draft.stage == null || capability == null}
        onPress={() => {
          if (draft.stage == null) return;
          void runAsync(
            "add",
            () => requestConditionDraft("field", draft.stage!),
            (condition) =>
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
