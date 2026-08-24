import { useEffect } from "react";
import { Button, Label, ListBox, Select } from "@heroui/react";
import { Plus, TrashBin } from "@gravity-ui/icons";
import type {
  RuleAction,
  RuleActionCapabilityViewModel,
  RuleDraft,
  RuleStageCapabilityViewModel,
  MessageStage,
} from "@/generated/rust-types";
import { ActionFields } from "./action-fields";
import {
  actionKind,
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
  capabilities,
  draftStage,
}: {
  action: RuleAction;
  onChange: (update: ActionUpdate) => void;
  asyncStateKey: string;
  onAsyncStateChange: AsyncStateChange;
  capabilities: RuleActionCapabilityViewModel[];
  draftStage: MessageStage;
}) {
  const runAsync = useAsyncRequestSlots(asyncStateKey, onAsyncStateChange);
  const capability = capabilities.find(
    (candidate) => candidate.kind === actionKind(action),
  );
  useEffect(() => {
    if (
      capability?.traffic_direction == null ||
      (action.type !== "throttle" && action.type !== "intermittent") ||
      action.direction === capability.traffic_direction
    ) {
      return;
    }
    onChange((current) =>
      current.type === "throttle" || current.type === "intermittent"
        ? { ...current, direction: capability.traffic_direction! }
        : current,
    );
  }, [action, capability, onChange]);
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
              () => requestActionDraft(kind as ActionKind, draftStage),
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
              {capabilities.map(({ kind }) => (
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
      {capability ? (
        <ActionFields
          action={action}
          trafficDirection={capability.traffic_direction ?? undefined}
          onChange={onChange}
          asyncStateKey={asyncStateKey}
          onAsyncStateChange={onAsyncStateChange}
        />
      ) : (
        <p className="text-sm text-[var(--telemetry-danger)]" role="alert">
          当前动作不支持所选阶段或所在位置，请改为下拉框中的可用动作。
        </p>
      )}
    </div>
  );
}

export function ActionsEditor({
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
        <p className="text-sm text-[var(--telemetry-danger)]" role="alert">
          {fieldErrors.actions.join("；")}
        </p>
      )}
      {draft.actions.map((action, index) => {
        const rowError = errorText(fieldErrors, `actions.${index}`);
        const available = (capability?.actions ?? []).filter(
          (candidate) => !candidate.terminal || index === draft.actions.length - 1,
        );
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
              capabilities={available}
              draftStage={draft.stage!}
            />
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
      })}
      <Button
        variant="outline"
        isDisabled={
          draft.stage == null ||
          capability == null ||
          draft.actions.some((action) => action.type === "terminal")
        }
        onPress={() => {
          if (draft.stage == null || capability == null) return;
          const defaultKind = capability.actions.find(
            (candidate) => candidate.kind === "delay",
          )?.kind ?? capability.actions[0]?.kind;
          if (defaultKind == null) return;
          void runAsync(
            "add",
            () => requestActionDraft(defaultKind, draft.stage!),
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
