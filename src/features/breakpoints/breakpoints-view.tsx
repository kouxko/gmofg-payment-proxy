"use client";

import { useMemo, useState } from "react";
import {
  Alert,
  Button,
  Chip,
  Drawer,
  FieldError,
  Label,
  NumberField,
  Select,
  ListBox,
  Spinner,
  Tabs,
  TextArea,
  TextField,
  toast,
} from "@heroui/react";
import { Copy, SlidersVertical } from "@gravity-ui/icons";
import type {
  BreakpointActionOptionViewModel,
  BreakpointDecision,
  BreakpointDetailViewModel,
  BreakpointDraft,
  BreakpointSummaryViewModel,
  FieldValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import {
  appErrorViewModel,
  callCommand,
  errorMessage,
} from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { formatTimestamp, toneColor } from "@/lib/format";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";

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
  const queue = useIpcQuery<BreakpointSummaryViewModel[]>(
    "breakpoint-query",
    () => callCommand(commands.breakpointQuery(null)),
  );
  useAppEventRefresh(
    ["breakpoint_queued", "breakpoint_resolved", "snapshot_required"],
    queue.refresh,
  );
  const [selectedId, setSelectedId] = useState<string | undefined>(() => {
    const requested = searchParams.get("breakpointId");
    return requested ?? undefined;
  });
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
  const [resolveDrawerOpen, setResolveDrawerOpen] = useState(false);
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
  const activeDecisionKind = selectedAction?.kind;

  function selectDecision(kind: BreakpointDecision["kind"]) {
    const action = detail.data?.available_actions.find(
      (candidate) => candidate.kind === kind,
    );
    if (!action) return;
    setDecisionKind(action.kind);
    setDelayMs(action.default_delay_ms ?? undefined);
    setHttpStatus(action.default_http_status ?? undefined);
    setContentLengthDelta(
      action.default_content_length_delta ?? undefined,
    );
    setTruncateAt(action.default_truncate_at ?? undefined);
  }

  const bodyText =
    (effectiveSelectedId && bodyEdits[effectiveSelectedId]) ??
    detail.data?.effective.body_text ??
    "";
  const validation =
    validationState &&
    validationState.breakpointId === effectiveSelectedId
      ? validationState.result
      : undefined;
  const validationError = (field: string) =>
    validation?.field_errors[field]?.join("；");

  function recordCommandFieldErrors(reason: unknown) {
    const errors = appErrorViewModel(reason)?.field_errors;
    if (!effectiveSelectedId || !errors || Object.keys(errors).length === 0) {
      return;
    }
    setValidationState({
      breakpointId: effectiveSelectedId,
      result: { valid: false, field_errors: errors, warnings: [] },
    });
  }

  const draft = useMemo<BreakpointDraft | undefined>(() => {
    if (!detail.data) return;
    return {
      breakpoint_id: detail.data.summary.breakpoint_id,
      expected_revision: detail.data.summary.revision,
      message: { ...detail.data.effective, body_text: bodyText },
    };
  }, [bodyText, detail.data]);

  async function formatJson() {
    if (!draft || editorPending || resolvePending) return;
    setEditorPending("format");
    try {
      const result = await callCommand(commands.breakpointFormatJson(draft));
      setBodyEdits((current) => ({
        ...current,
        [result.breakpoint_id]: result.message.body_text ?? "",
      }));
      setValidationState(undefined);
    } catch (reason) {
      recordCommandFieldErrors(reason);
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setEditorPending(undefined);
    }
  }

  async function restoreOriginal() {
    if (!selectedSummary || editorPending || resolvePending) return;
    setEditorPending("restore");
    try {
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
    } catch (reason) {
      recordCommandFieldErrors(reason);
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setEditorPending(undefined);
    }
  }

  async function validate() {
    if (!draft || !selectedSummary || editorPending || resolvePending) return;
    setEditorPending("validate");
    try {
      const result = await callCommand(
        commands.breakpointValidate(draft, selectedSummary.runtime_epoch),
      );
      setValidationState({ breakpointId: draft.breakpoint_id, result });
    } catch (reason) {
      recordCommandFieldErrors(reason);
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setEditorPending(undefined);
    }
  }

  async function resolve(kind: BreakpointDecision["kind"]) {
    const action = detail.data?.available_actions.find(
      (candidate) => candidate.kind === kind,
    );
    if (!draft || !selectedSummary || !action?.enabled || resolvePending) {
      return;
    }
    setResolvePending(true);
    try {
      const decision = buildBreakpointDecision(draft, action, {
        delayMs,
        httpStatus,
        contentLengthDelta,
        truncateAt,
      });
      const result = await callCommand(
        commands.breakpointResolve(selectedSummary.runtime_epoch, decision),
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
      setResolveDrawerOpen(false);
    } catch (reason) {
      recordCommandFieldErrors(reason);
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setResolvePending(false);
    }
  }

  return (
    <section className="grid h-full grid-cols-[290px_minmax(0,1fr)_260px] max-[1280px]:grid-cols-[250px_minmax(0,1fr)] max-[820px]:grid-cols-1">
      <aside className="overflow-auto border-r border-[var(--telemetry-line)] p-3 max-[820px]:max-h-64 max-[820px]:border-r-0 max-[820px]:border-b">
        <div className="mb-3 flex items-center">
          <h1 className="text-lg font-semibold">
            暂停队列 ({queue.data?.length ?? 0})
          </h1>
          <Button
            className="ml-auto"
            isIconOnly
            size="sm"
            variant="ghost"
            aria-label="刷新断点队列"
            onPress={() => void queue.refresh()}
          >
            <SlidersVertical className="size-4" />
          </Button>
        </div>
        <div className="space-y-3">
          {queue.error && (
            <Alert status="danger">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>断点队列读取失败</Alert.Title>
                <Alert.Description>{queue.error}</Alert.Description>
              </Alert.Content>
              <Button
                size="sm"
                variant="outline"
                onPress={() => void queue.refresh()}
              >
                重试
              </Button>
            </Alert>
          )}
          {(queue.data ?? []).map((item) => (
            <Button
              key={item.breakpoint_id}
              variant={
                item.breakpoint_id === effectiveSelectedId
                  ? "primary"
                  : "outline"
              }
              className="h-auto w-full justify-start px-3 py-3 text-left"
              onPress={() => setSelectedId(item.breakpoint_id)}
            >
              <div className="min-w-0 flex-1 space-y-2">
                <div className="flex items-center gap-2">
                  <Chip
                    size="sm"
                    color={toneColor(item.ui_tone)}
                    variant="soft"
                  >
                    {item.stage === "request" ? "请求断点" : "响应断点"}
                  </Chip>
                  <span>{item.terminal_ip}</span>
                  <span className="ml-auto">
                    {item.channel === "transaction" ? "交易" : "DLL"}
                  </span>
                </div>
                <div className="truncate font-mono text-xs">
                  {item.method} {item.target}
                </div>
                <div className="flex text-xs">
                  <span>{formatTimestamp(item.waiting_since)}</span>
                  <span className="ml-auto">
                    {item.certificate_fingerprint_suffix}
                  </span>
                </div>
              </div>
            </Button>
          ))}
          {!queue.isLoading && !queue.error && queue.data?.length === 0 && (
            <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
              当前没有待处理断点
            </p>
          )}
          {queue.isLoading && <Spinner aria-label="正在加载断点队列" />}
        </div>
      </aside>

      <div className="min-w-0 overflow-auto p-5">
        {selectedSummary && detail.error ? (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>断点详情读取失败</Alert.Title>
              <Alert.Description>{detail.error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void detail.refresh()}
            >
              重试
            </Button>
          </Alert>
        ) : selectedSummary && detail.isLoading ? (
          <div className="grid h-full place-items-center">
            <Spinner aria-label="正在读取断点详情" />
          </div>
        ) : !detail.data ? (
          <div className="grid h-full place-items-center text-sm text-[var(--telemetry-muted)]">
            选择一条待处理断点
          </div>
        ) : (
          <div className="space-y-4">
            <div className="flex min-w-0 flex-wrap items-center gap-x-5 gap-y-2">
              <h2 className="min-w-0 text-lg font-semibold">
                {detail.data.summary.title}
              </h2>
              <span>终端 IP {detail.data.summary.terminal_ip}</span>
              <span>
                {detail.data.summary.channel === "transaction" ? "交易通道" : "DLL 通道"}
              </span>
              <span className="ml-auto max-w-full truncate font-mono text-xs">
                请求 ID {detail.data.summary.session_id}
              </span>
            </div>

            <div className="grid grid-cols-2 gap-5 max-[1100px]:grid-cols-1">
              <div>
                <h3 className="mb-2 font-semibold">原始报文</h3>
                <Tabs defaultSelectedKey="json">
                  <Tabs.ListContainer>
                    <Tabs.List aria-label="原始报文查看">
                      <Tabs.Tab id="json">
                        JSON
                        <Tabs.Indicator />
                      </Tabs.Tab>
                      <Tabs.Tab id="headers">
                        请求头
                        <Tabs.Indicator />
                      </Tabs.Tab>
                      <Tabs.Tab id="bytes">
                        原始字节
                        <Tabs.Indicator />
                      </Tabs.Tab>
                    </Tabs.List>
                  </Tabs.ListContainer>
                  <Tabs.Panel id="json" className="pt-3">
                    <TextArea
                      aria-label="原始 JSON"
                      className="min-h-[430px] font-mono text-xs"
                      value={detail.data.original.body_text ?? ""}
                      readOnly
                    />
                  </Tabs.Panel>
                  <Tabs.Panel id="headers" className="pt-3">
                    <TextArea
                      aria-label="原始请求头"
                      className="min-h-[430px] font-mono text-xs"
                      value={JSON.stringify(detail.data.original.headers, null, 2)}
                      readOnly
                    />
                  </Tabs.Panel>
                  <Tabs.Panel id="bytes" className="pt-3">
                    <TextArea
                      aria-label="原始字节"
                      className="min-h-[430px] font-mono text-xs"
                      value={detail.data.original.body_bytes.join(" ")}
                      readOnly
                    />
                  </Tabs.Panel>
                </Tabs>
              </div>
              <div>
                <h3 className="mb-2 font-semibold">有效报文</h3>
                <Tabs defaultSelectedKey="json">
                  <Tabs.ListContainer>
                    <Tabs.List aria-label="有效报文编辑">
                      <Tabs.Tab id="json">
                        JSON
                        <Tabs.Indicator />
                      </Tabs.Tab>
                      <Tabs.Tab id="headers">
                        请求头
                        <Tabs.Indicator />
                      </Tabs.Tab>
                      <Tabs.Tab id="bytes">
                        原始字节
                        <Tabs.Indicator />
                      </Tabs.Tab>
                    </Tabs.List>
                  </Tabs.ListContainer>
                  <Tabs.Panel id="json" className="pt-3">
                    <TextField
                      isInvalid={Boolean(
                        validationError("message.body_text") ??
                          validationError("message"),
                      )}
                    >
                      <TextArea
                        aria-label="有效 JSON"
                        className="min-h-[430px] font-mono text-xs"
                        value={bodyText}
                        onChange={(event) => {
                          if (!effectiveSelectedId) return;
                          setBodyEdits((current) => ({
                            ...current,
                            [effectiveSelectedId]: event.target.value,
                          }));
                          setValidationState(undefined);
                        }}
                      />
                      {(validationError("message.body_text") ??
                        validationError("message")) && (
                        <FieldError>
                          {validationError("message.body_text") ??
                            validationError("message")}
                        </FieldError>
                      )}
                    </TextField>
                  </Tabs.Panel>
                  <Tabs.Panel id="headers" className="pt-3">
                    <TextArea
                      aria-label="有效请求头"
                      className="min-h-[430px] font-mono text-xs"
                      value={JSON.stringify(detail.data.effective.headers, null, 2)}
                      readOnly
                    />
                    {validationError("message.headers") && (
                      <p className="mt-2 text-sm text-danger">
                        {validationError("message.headers")}
                      </p>
                    )}
                  </Tabs.Panel>
                  <Tabs.Panel id="bytes" className="pt-3">
                    <TextArea
                      aria-label="有效原始字节"
                      className="min-h-[430px] font-mono text-xs"
                      value={detail.data.effective.body_bytes.join(" ")}
                      readOnly
                    />
                  </Tabs.Panel>
                </Tabs>
              </div>
            </div>

            <div className="flex flex-wrap gap-3">
              <Button
                variant="outline"
                isDisabled={Boolean(editorPending) || resolvePending}
                onPress={() => void formatJson()}
              >
                {editorPending === "format" ? "正在格式化…" : "格式化 JSON"}
              </Button>
              <Button
                variant="outline"
                onPress={() => void navigator.clipboard.writeText(bodyText)}
              >
                <Copy className="size-4" />
                复制
              </Button>
              <Button
                variant="outline"
                isDisabled={Boolean(editorPending) || resolvePending}
                onPress={() => void restoreOriginal()}
              >
                {editorPending === "restore" ? "正在恢复…" : "恢复原始报文"}
              </Button>
              <Button
                className="ml-auto max-[1280px]:ml-0"
                variant="outline"
                isDisabled={Boolean(editorPending) || resolvePending}
                onPress={() => void validate()}
              >
                {editorPending === "validate" ? "正在校验…" : "由 Rust 校验"}
              </Button>
              <Drawer
                isOpen={resolveDrawerOpen}
                onOpenChange={(open) => {
                  if (!open && resolvePending) return;
                  setResolveDrawerOpen(open);
                }}
              >
                <Button
                  className="hidden max-[1280px]:inline-flex"
                  variant="outline"
                >
                  处理断点
                </Button>
                <Drawer.Backdrop isDismissable={!resolvePending}>
                  <Drawer.Content placement="right">
                    <Drawer.Dialog>
                      <Drawer.Header>
                        <Drawer.Heading>处理断点</Drawer.Heading>
                      </Drawer.Header>
                      <Drawer.Body className="space-y-4">
                        <Select
                          aria-label="移动端断点处理方式"
                          selectedKey={activeDecisionKind}
                          onSelectionChange={(key) =>
                            selectDecision(
                              key as BreakpointDecision["kind"],
                            )
                          }
                        >
                          <Select.Trigger>
                            <Select.Value />
                            <Select.Indicator />
                          </Select.Trigger>
                          <Select.Popover>
                            <ListBox>
                              {detail.data?.available_actions.map((action) => (
                                <ListBox.Item
                                  key={action.kind}
                                  id={action.kind}
                                  isDisabled={!action.enabled}
                                >
                                  {action.label}
                                </ListBox.Item>
                              ))}
                            </ListBox>
                          </Select.Popover>
                        </Select>
                        {activeDecisionKind === "delay" &&
                          selectedAction?.default_delay_ms != null && (
                          <NumberField
                            value={
                              delayMs ?? selectedAction.default_delay_ms
                            }
                            minValue={0}
                            onChange={setDelayMs}
                          >
                            <Label>延迟毫秒</Label>
                            <NumberField.Group className="w-full">
                              <NumberField.DecrementButton />
                              <NumberField.Input />
                              <NumberField.IncrementButton />
                            </NumberField.Group>
                          </NumberField>
                          )}
                        {activeDecisionKind === "custom_http_status" &&
                          selectedAction?.default_http_status != null && (
                          <NumberField
                            value={
                              httpStatus ??
                              selectedAction.default_http_status
                            }
                            minValue={100}
                            maxValue={599}
                            onChange={setHttpStatus}
                          >
                            <Label>HTTP 状态码</Label>
                            <NumberField.Group className="w-full">
                              <NumberField.DecrementButton />
                              <NumberField.Input />
                              <NumberField.IncrementButton />
                            </NumberField.Group>
                          </NumberField>
                          )}
                        {activeDecisionKind === "wrong_content_length" &&
                          selectedAction?.default_content_length_delta !=
                            null && (
                          <NumberField
                            value={
                              contentLengthDelta ??
                              selectedAction.default_content_length_delta
                            }
                            onChange={setContentLengthDelta}
                          >
                            <Label>Content-Length 差值</Label>
                            <NumberField.Group className="w-full">
                              <NumberField.DecrementButton />
                              <NumberField.Input />
                              <NumberField.IncrementButton />
                            </NumberField.Group>
                          </NumberField>
                          )}
                        {activeDecisionKind === "truncate" &&
                          selectedAction?.default_truncate_at != null && (
                          <NumberField
                            value={
                              truncateAt ??
                              selectedAction.default_truncate_at
                            }
                            minValue={0}
                            onChange={setTruncateAt}
                          >
                            <Label>截断字节位置</Label>
                            <NumberField.Group className="w-full">
                              <NumberField.DecrementButton />
                              <NumberField.Input />
                              <NumberField.IncrementButton />
                            </NumberField.Group>
                          </NumberField>
                          )}
                      </Drawer.Body>
                      <Drawer.Footer>
                        <Button
                          slot="close"
                          variant="outline"
                          isDisabled={resolvePending}
                        >
                          取消
                        </Button>
                        <Button
                          variant="primary"
                          isDisabled={
                            resolvePending ||
                            !detail.data?.can_resolve ||
                            validation?.valid === false
                          }
                          onPress={() => {
                            if (activeDecisionKind) {
                              void resolve(activeDecisionKind);
                            }
                          }}
                        >
                          {resolvePending ? "正在处理…" : "执行所选处理"}
                        </Button>
                      </Drawer.Footer>
                    </Drawer.Dialog>
                  </Drawer.Content>
                </Drawer.Backdrop>
              </Drawer>
            </div>

            {validation && (
              <Alert status={validation.valid ? "success" : "danger"}>
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>
                    {validation.valid ? "报文校验通过" : "报文校验失败"}
                  </Alert.Title>
                  <Alert.Description>
                    {validation.valid
                      ? validation.warnings.join("；") || "JSON、Shift-JIS 和报文长度有效。"
                      : Object.values(validation.field_errors).flat().join("；")}
                  </Alert.Description>
                </Alert.Content>
              </Alert>
            )}
          </div>
        )}
      </div>

      <aside className="overflow-auto border-l border-[var(--telemetry-line)] p-4 max-[1280px]:hidden">
        <h2 className="mb-5 text-lg font-semibold">处理方式</h2>
        <Select
          aria-label="断点处理方式"
          selectedKey={activeDecisionKind}
          onSelectionChange={(key) =>
            selectDecision(key as BreakpointDecision["kind"])
          }
        >
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              {detail.data?.available_actions.map((action) => (
                <ListBox.Item
                  key={action.kind}
                  id={action.kind}
                  isDisabled={!action.enabled}
                >
                  {action.label}
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
        <div className="mt-4 space-y-3">
          {activeDecisionKind === "delay" &&
            selectedAction?.default_delay_ms != null && (
            <NumberField
              value={delayMs ?? selectedAction.default_delay_ms}
              minValue={0}
              onChange={setDelayMs}
            >
              <Label>延迟毫秒</Label>
              <NumberField.Group className="w-full">
                <NumberField.DecrementButton />
                <NumberField.Input />
                <NumberField.IncrementButton />
              </NumberField.Group>
            </NumberField>
            )}
          {activeDecisionKind === "custom_http_status" &&
            selectedAction?.default_http_status != null && (
            <NumberField
              value={httpStatus ?? selectedAction.default_http_status}
              minValue={100}
              maxValue={599}
              onChange={setHttpStatus}
            >
              <Label>HTTP 状态码</Label>
              <NumberField.Group className="w-full">
                <NumberField.DecrementButton />
                <NumberField.Input />
                <NumberField.IncrementButton />
              </NumberField.Group>
            </NumberField>
            )}
          {activeDecisionKind === "wrong_content_length" &&
            selectedAction?.default_content_length_delta != null && (
            <NumberField
              value={
                contentLengthDelta ??
                selectedAction.default_content_length_delta
              }
              onChange={setContentLengthDelta}
            >
              <Label>Content-Length 差值</Label>
              <NumberField.Group className="w-full">
                <NumberField.DecrementButton />
                <NumberField.Input />
                <NumberField.IncrementButton />
              </NumberField.Group>
            </NumberField>
            )}
          {activeDecisionKind === "truncate" &&
            selectedAction?.default_truncate_at != null && (
            <NumberField
              value={truncateAt ?? selectedAction.default_truncate_at}
              minValue={0}
              onChange={setTruncateAt}
            >
              <Label>截断字节位置</Label>
              <NumberField.Group className="w-full">
                <NumberField.DecrementButton />
                <NumberField.Input />
                <NumberField.IncrementButton />
              </NumberField.Group>
            </NumberField>
            )}
        </div>
        <div className="mt-8 space-y-3">
          <Button
            fullWidth
            variant="primary"
            isDisabled={
              resolvePending ||
              !detail.data?.can_resolve ||
              validation?.valid === false
            }
            onPress={() => {
              if (activeDecisionKind) void resolve(activeDecisionKind);
            }}
          >
            {resolvePending ? "正在处理…" : "执行所选处理"}
          </Button>
          {detail.data?.available_actions
            .filter((action) => action.kind === "forward_original")
            .map((action) => (
              <Button
                key={action.kind}
                fullWidth
                variant="outline"
                isDisabled={resolvePending || !action.enabled}
                onPress={() => void resolve(action.kind)}
              >
                {action.label}
              </Button>
            ))}
          {detail.data?.available_actions
            .filter(
              (action) => action.kind === "disconnect_before_upstream",
            )
            .map((action) => (
              <Button
                key={action.kind}
                fullWidth
                variant="danger-soft"
                isDisabled={resolvePending || !action.enabled}
                onPress={() => void resolve(action.kind)}
              >
                {action.label}
              </Button>
            ))}
        </div>
      </aside>
    </section>
  );
}
