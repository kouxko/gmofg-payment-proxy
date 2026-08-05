"use client";

/** 人工断点容器：Rust 持有暂停任务；此处只管理草稿、校验和决策 IPC。 */
import { useMemo, useState } from "react";
import { toast } from "@heroui/react";
import type {
  BreakpointActionOptionViewModel,
  BreakpointDecision,
  BreakpointDetailViewModel,
  BreakpointDraft,
  BreakpointSummaryViewModel,
  FieldValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { toneColor } from "@/lib/format";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { BreakpointActionPanel } from "./breakpoint-action-panel";
import { BreakpointEditorPanel } from "./breakpoint-editor-panel";
import { BreakpointQueuePanel } from "./breakpoint-queue-panel";

export function buildBreakpointDecision(
  draft: BreakpointDraft,
  action: BreakpointActionOptionViewModel,
  parameters: {
    delayMs?: number;
    httpStatus?: number;
    contentLengthDelta?: number;
    truncateAt?: number;
  },
): BreakpointDecision {
  return {
    breakpoint_id: draft.breakpoint_id,
    expected_revision: draft.expected_revision,
    kind: action.kind,
    message: draft.message,
    delay_ms: parameters.delayMs ?? action.default_delay_ms,
    http_status: parameters.httpStatus ?? action.default_http_status,
    content_length_delta:
      parameters.contentLengthDelta ?? action.default_content_length_delta,
    truncate_at: parameters.truncateAt ?? action.default_truncate_at,
  };
}

export function BreakpointsView() {
  const { searchParams } = useWorkspaceNavigation();
  const requestedId = searchParams.get("breakpointId") ?? undefined;
  const queue = useIpcQuery<BreakpointSummaryViewModel[]>(
    "breakpoint-query",
    () => callCommand(commands.breakpointQuery(null)),
  );
  useAppEventRefresh(
    ["breakpoint_queued", "breakpoint_resolved", "snapshot_required"],
    queue.refresh,
  );
  const [selection, setSelection] = useState(() => ({
    routeBreakpointId: requestedId,
    selectedId: requestedId,
  }));
  const selectedId =
    selection.routeBreakpointId === requestedId
      ? selection.selectedId
      : requestedId;
  const setSelectedId = (value?: string) =>
    setSelection({ routeBreakpointId: requestedId, selectedId: value });
  const [bodyEdits, setBodyEdits] = useState<Record<string, string>>({});
  const [validationState, setValidationState] = useState<{
    breakpointId: string;
    result: FieldValidationViewModel;
  }>();
  const [decisionKind, setDecisionKind] =
    useState<BreakpointDecision["kind"]>();
  const [delayMs, setDelayMs] = useState<number>();
  const [httpStatus, setHttpStatus] = useState<number>();
  const [contentLengthDelta, setContentLengthDelta] = useState<number>();
  const [truncateAt, setTruncateAt] = useState<number>();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [resolvePending, setResolvePending] = useState(false);
  const [editorPending, setEditorPending] = useState<
    "format" | "restore" | "validate"
  >();
  const effectiveSelectedId = selectedId ?? queue.data?.[0]?.breakpoint_id;
  const selectedSummary = queue.data?.find(
    (item) => item.breakpoint_id === effectiveSelectedId,
  );
  const detail = useIpcQuery<BreakpointDetailViewModel>(
    `breakpoint-detail:${selectedSummary?.breakpoint_id ?? "none"}`,
    () =>
      callCommand(
        commands.breakpointGet(
          selectedSummary!.breakpoint_id,
          selectedSummary!.runtime_epoch,
        ),
      ),
    undefined,
    { enabled: Boolean(selectedSummary) },
  );
  const selectedAction =
    detail.data?.available_actions.find(
      (action) => action.kind === decisionKind,
    ) ?? detail.data?.available_actions[0];
  const bodyText =
    (effectiveSelectedId && bodyEdits[effectiveSelectedId]) ??
    detail.data?.effective.body_text ??
    "";
  const validation =
    validationState?.breakpointId === effectiveSelectedId
      ? validationState?.result
      : undefined;
  const validationError = (field: string) =>
    validation?.field_errors[field]?.join("；");
  const draft = useMemo<BreakpointDraft | undefined>(
    () =>
      detail.data
        ? {
            breakpoint_id: detail.data.summary.breakpoint_id,
            expected_revision: detail.data.summary.revision,
            message: { ...detail.data.effective, body_text: bodyText },
          }
        : undefined,
    [bodyText, detail.data],
  );

  function recordErrors(reason: unknown) {
    const errors = appErrorViewModel(reason)?.field_errors;
    if (effectiveSelectedId && errors && Object.keys(errors).length > 0)
      setValidationState({
        breakpointId: effectiveSelectedId,
        result: { valid: false, field_errors: errors, warnings: [] },
      });
  }
  function selectDecision(kind: BreakpointDecision["kind"]) {
    const action = detail.data?.available_actions.find(
      (item) => item.kind === kind,
    );
    if (!action) return;
    setDecisionKind(action.kind);
    setDelayMs(action.default_delay_ms ?? undefined);
    setHttpStatus(action.default_http_status ?? undefined);
    setContentLengthDelta(action.default_content_length_delta ?? undefined);
    setTruncateAt(action.default_truncate_at ?? undefined);
  }
  async function runEditor(kind: "format" | "restore" | "validate") {
    if (
      !selectedSummary ||
      editorPending ||
      resolvePending ||
      (kind !== "restore" && !draft)
    )
      return;
    setEditorPending(kind);
    try {
      if (kind === "format") {
        const result = await callCommand(commands.breakpointFormatJson(draft!));
        setBodyEdits((current) => ({
          ...current,
          [result.breakpoint_id]: result.message.body_text ?? "",
        }));
        setValidationState(undefined);
      } else if (kind === "restore") {
        const result = await callCommand(
          commands.breakpointRestoreOriginal(
            selectedSummary.breakpoint_id,
            selectedSummary.runtime_epoch,
          ),
        );
        setBodyEdits((current) => ({
          ...current,
          [result.breakpoint_id]: result.message.body_text ?? "",
        }));
        setValidationState(undefined);
      } else {
        const result = await callCommand(
          commands.breakpointValidate(draft!, selectedSummary.runtime_epoch),
        );
        setValidationState({ breakpointId: draft!.breakpoint_id, result });
      }
    } catch (reason) {
      recordErrors(reason);
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setEditorPending(undefined);
    }
  }
  async function resolve(kind: BreakpointDecision["kind"]) {
    const action = detail.data?.available_actions.find(
      (item) => item.kind === kind,
    );
    if (!draft || !selectedSummary || !action?.enabled || resolvePending)
      return;
    setResolvePending(true);
    try {
      const result = await callCommand(
        commands.breakpointResolve(
          selectedSummary.runtime_epoch,
          buildBreakpointDecision(draft, action, {
            delayMs,
            httpStatus,
            contentLengthDelta,
            truncateAt,
          }),
        ),
      );
      toast(result.state_text, { variant: toneColor(result.ui_tone) });
      setBodyEdits((current) => {
        const next = { ...current };
        delete next[draft.breakpoint_id];
        return next;
      });
      setValidationState(undefined);
      detail.invalidate();
      setSelectedId(undefined);
      await queue.refresh();
      setDrawerOpen(false);
    } catch (reason) {
      recordErrors(reason);
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setResolvePending(false);
    }
  }

  const actionProps = {
    actions: detail.data?.available_actions ?? [],
    selected: selectedAction,
    delayMs,
    httpStatus,
    contentLengthDelta,
    truncateAt,
    resolvePending,
    canResolve: detail.data?.can_resolve ?? false,
    validationValid: validation?.valid,
    onSelect: selectDecision,
    onDelayChange: setDelayMs,
    onHttpStatusChange: setHttpStatus,
    onContentLengthDeltaChange: setContentLengthDelta,
    onTruncateAtChange: setTruncateAt,
    onResolve: (kind: BreakpointDecision["kind"]) => void resolve(kind),
  };
  return (
    <section className="grid h-full grid-cols-[290px_minmax(0,1fr)_260px] max-[1280px]:grid-cols-[250px_minmax(0,1fr)] max-[820px]:grid-cols-1">
      <BreakpointQueuePanel
        data={queue.data}
        error={queue.error}
        isLoading={queue.isLoading}
        selectedId={effectiveSelectedId}
        onRefresh={() => void queue.refresh()}
        onSelect={setSelectedId}
      />
      <BreakpointEditorPanel
        hasSelection={Boolean(selectedSummary)}
        detail={detail}
        bodyText={bodyText}
        editorPending={editorPending}
        resolvePending={resolvePending}
        validation={validation}
        validationError={validationError}
        drawerOpen={drawerOpen}
        actionProps={actionProps}
        onBodyChange={(value) => {
          if (!effectiveSelectedId) return;
          setBodyEdits((current) => ({
            ...current,
            [effectiveSelectedId]: value,
          }));
          setValidationState(undefined);
        }}
        onFormat={() => void runEditor("format")}
        onRestore={() => void runEditor("restore")}
        onValidate={() => void runEditor("validate")}
        onDrawerChange={(open) => {
          if (!open && resolvePending) return;
          setDrawerOpen(open);
        }}
        onResolve={(kind) => void resolve(kind)}
      />
      <BreakpointActionPanel {...actionProps} />
    </section>
  );
}
