import { Button, FieldError, Label, ListBox, Select } from "@heroui/react";
import { Plus, TrashBin } from "@gravity-ui/icons";
import type { RuleAction, RuleDraft } from "@/generated/rust-types";
import { ActionFields } from "./action-fields";
import {
  actionKind,
  actionKinds,
  actionLabels,
  errorText,
  requestActionDraft,
  useAsyncRequestSlots,
  type ActionKind,
  type ActionUpdate,
  type AsyncStateChange,
  type RuleDraftChange,
} from "./rule-editor-model";

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
  const runAsync = useAsyncRequestSlots(asyncStateKey, onAsyncStateChange);
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
      <ActionFields
        action={action}
        onChange={onChange}
        asyncStateKey={asyncStateKey}
        onAsyncStateChange={onAsyncStateChange}
      />
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
      {draft.actions.map((action, index) => {
        const rowError = errorText(fieldErrors, `actions.${index}`);
        return (
          <div
            key={`${editorKey}:${draft.actions.length}:${index}:${actionKind(action)}`}
            className="rounded-xl border border-[var(--telemetry-line)] p-3"
          >
            <div className="mb-3 flex min-h-8 items-center justify-between gap-3">
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
            {rowError && <FieldError className="mt-2">{rowError}</FieldError>}
          </div>
        );
      })}
      <Button
        variant="outline"
        onPress={() => {
          void runAsync(
            "add",
            () => requestActionDraft("delay"),
            (action) =>
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
