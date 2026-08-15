"use client";

/** Socket 抓包的数据装配层：工作区、分页、详情、事件刷新和清空确认均在此收口。 */
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Alert, AlertDialog, Button, Spinner, toast } from "@heroui/react";
import type {
  SocketCaptureDetailViewModel,
  SocketCapturePageViewModel,
  SocketCaptureRowViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { SocketCaptureDetail } from "./socket-capture-detail";
import { SocketCaptureList } from "./socket-capture-list";
import {
  defaultSocketCaptureQuery,
  validateOperationResult,
  validateSelectedWorkspace,
  validateSocketCaptureDetail,
  validateSocketCapturePage,
} from "./socket-capture-model";

export function SocketCaptureView() {
  const [pageState, setPageState] = useState({ workspaceId: "", page: 1 });
  const [selection, setSelection] = useState({ workspaceId: "", captureId: "" });
  const [clearOpen, setClearOpen] = useState(false);
  const [clearPending, setClearPending] = useState(false);
  const [clearWorkspaceId, setClearWorkspaceId] = useState("");
  const clearAttempt = useRef(0);
  const currentWorkspaceId = useRef<string | undefined>(undefined);
  const clearButtonId = "socket-capture-clear-button";
  const workspaces = useIpcQuery<unknown>("socket-capture-workspaces", () =>
    callCommand(commands.workspaceList()),
  );
  // refresh 期间 useIpcQuery 会保留旧 data；此处仍必须停用 Socket 查询，不能让旧
  // 工作区快照在 workspace_changed 后继续驱动后端请求。
  const selectedWorkspace = !workspaces.isLoading && !workspaces.error
    ? validateSelectedWorkspace(workspaces.data)
    : undefined;
  const workspaceId = selectedWorkspace?.id;
  useLayoutEffect(() => {
    currentWorkspaceId.current = workspaceId;
  }, [workspaceId]);
  const pageNumber = pageState.workspaceId === workspaceId ? pageState.page : 1;
  const query = useMemo(
    () => workspaceId
      ? { ...defaultSocketCaptureQuery(workspaceId), page: { page: pageNumber, page_size: 50 } }
      : undefined,
    [pageNumber, workspaceId],
  );
  const queryKey = query ? JSON.stringify(query) : "no-selected-workspace";
  const page = useIpcQuery<SocketCapturePageViewModel>(
    `socket-capture-query:${queryKey}`,
    async () => {
      const raw = await callCommand(commands.socketCaptureQuery(query!));
      const valid = validateSocketCapturePage(raw, workspaceId!);
      if (!valid) throw new Error("Socket 抓包列表返回了不一致或畸形的数据");
      return valid;
    },
    undefined,
    { enabled: Boolean(query) },
  );
  useAppEventRefresh(["workspace_changed"], workspaces.refresh);
  useAppEventRefresh(
    ["socket_capture_completed", "snapshot_required", "workspace_changed"],
    page.refresh,
    { paused: !query },
  );

  useEffect(() => {
    if (!workspaceId || !page.data) return;
    const lastPage = Math.max(1, page.data.total_pages);
    if (page.data.page <= lastPage) return;
    const task = window.setTimeout(
      () => setPageState({ workspaceId, page: lastPage }),
      0,
    );
    return () => window.clearTimeout(task);
  }, [page.data, workspaceId]);

  const selectedId = selection.workspaceId === workspaceId ? selection.captureId : undefined;
  const selected = page.data?.rows.find((row) => row.capture_id === selectedId);
  const detailKey = selected && workspaceId
    ? [workspaceId, selected.capture_id, selected.runtime_epoch, selected.package.id, selected.package.version, selected.schema.id, selected.schema.version].join(":")
    : "none";
  const detailQuery = useIpcQuery<SocketCaptureDetailViewModel>(
    `socket-capture-detail:${detailKey}`,
    () => callCommand(commands.socketCaptureGetDetail(selected!.capture_id)),
    undefined,
    { enabled: Boolean(selected && workspaceId) },
  );
  const detail = selected && workspaceId
    ? validateSocketCaptureDetail(detailQuery.data, selected, workspaceId)
    : undefined;
  const malformed = detailQuery.data !== undefined && !detail;

  useEffect(() => {
    const workspaceChanged = selection.captureId && selection.workspaceId !== workspaceId;
    const rowDisappeared = selection.captureId && selection.workspaceId === workspaceId
      && Boolean(page.data) && !selected;
    if (!workspaceChanged && !rowDisappeared) return;
    const task = window.setTimeout(() => {
      setSelection({ workspaceId: "", captureId: "" });
      setClearOpen(false);
      detailQuery.invalidate();
      document.getElementById("socket-capture-list")?.focus();
    }, 0);
    return () => window.clearTimeout(task);
  }, [detailQuery, page.data, selected, selection, workspaceId]);

  useEffect(() => {
    if (!clearWorkspaceId || clearWorkspaceId === workspaceId) return;
    // 清空确认只属于打开它时的工作区。工作区一旦变化，立刻使正在等待的
    // 前端响应失效；后端还会再次核对精确 Workspace ID，形成双重保护。
    clearAttempt.current += 1;
    const task = window.setTimeout(() => {
      setClearOpen(false);
      setClearWorkspaceId("");
      setClearPending(false);
    }, 0);
    return () => window.clearTimeout(task);
  }, [clearWorkspaceId, workspaceId]);

  async function clearCaptures() {
    if (clearPending || !workspaceId || clearWorkspaceId !== workspaceId) return;
    const targetWorkspaceId = clearWorkspaceId;
    const attempt = clearAttempt.current + 1;
    clearAttempt.current = attempt;
    setClearPending(true);
    try {
      const result = validateOperationResult(
        await callCommand(commands.socketCaptureClear(targetWorkspaceId, true)),
      );
      if (clearAttempt.current !== attempt || currentWorkspaceId.current !== targetWorkspaceId) return;
      if (!result || result.entity_id !== null || result.revision !== null || result.requires_restart) {
        throw new Error("Socket 抓包清空命令返回了畸形结果");
      }
      if (!result.success || result.cancelled) {
        toast(result.message || "Socket 抓包未清空。", { variant: "danger" });
        return;
      }
      setSelection({ workspaceId: "", captureId: "" });
      detailQuery.invalidate();
      setPageState({ workspaceId, page: 1 });
      await page.refresh();
      if (clearAttempt.current !== attempt || currentWorkspaceId.current !== targetWorkspaceId) return;
      setClearOpen(false);
      setClearWorkspaceId("");
      toast("当前工作区的 Socket 抓包已清空。", { variant: "success" });
      requestAnimationFrame(() => document.getElementById(clearButtonId)?.focus());
    } catch (reason) {
      if (clearAttempt.current !== attempt || currentWorkspaceId.current !== targetWorkspaceId) return;
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      if (clearAttempt.current === attempt) setClearPending(false);
    }
  }

  if (workspaces.isLoading) {
    return <div className="grid h-full place-items-center"><Spinner aria-label="正在读取当前工作区" /></div>;
  }
  if (workspaces.error || !selectedWorkspace) {
    return (
      <div className="grid h-full place-items-center p-5">
        <Alert status="danger" className="max-w-xl">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>无法确定唯一的当前工作区</Alert.Title>
            <Alert.Description>{workspaces.error ?? "工作区列表无效、没有选中项或存在多个选中项；已停止查询 Socket 抓包。"}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={() => void workspaces.refresh()}>重试</Button>
        </Alert>
      </div>
    );
  }

  return (
    <section aria-label="Socket 抓包工作区" className="grid h-full min-h-0 grid-cols-1 grid-rows-1">
      <SocketCaptureList
        page={page.data}
        error={page.error}
        loading={page.isLoading}
        selectedId={selected?.capture_id}
        onSelect={(row: SocketCaptureRowViewModel) => setSelection({ workspaceId: selectedWorkspace.id, captureId: row.capture_id })}
        onPage={(nextPage) => setPageState({ workspaceId: selectedWorkspace.id, page: nextPage })}
        onRetry={() => void page.refresh()}
        onClear={() => {
          setClearWorkspaceId(selectedWorkspace.id);
          setClearOpen(true);
        }}
        clearButtonId={clearButtonId}
      />
      <SocketCaptureDetail
        selected={selected}
        detail={detail}
        error={detailQuery.error}
        malformed={malformed}
        loading={detailQuery.isLoading}
        onClose={() => {
          const focusId = selected?.capture_id;
          setSelection({ workspaceId: "", captureId: "" });
          detailQuery.invalidate();
          if (focusId) requestAnimationFrame(() => document.getElementById(`socket-capture-row-${focusId}`)?.focus());
        }}
        onRetry={() => void detailQuery.refresh()}
      />
      <AlertDialog
        isOpen={clearOpen && clearWorkspaceId === selectedWorkspace.id}
        onOpenChange={(open) => {
          if (clearPending) return;
          setClearOpen(open);
          if (!open) setClearWorkspaceId("");
        }}
      >
        <Button className="hidden" aria-hidden="true">打开 Socket 清空确认</Button>
        <AlertDialog.Backdrop>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header><AlertDialog.Heading>清空当前工作区的 Socket 抓包？</AlertDialog.Heading></AlertDialog.Header>
              <AlertDialog.Body>将删除当前选中工作区的全部 Socket 抓包详情。此操作不会停止 Listener，也不会删除 HTTP 抓包。</AlertDialog.Body>
              <AlertDialog.Footer>
                <Button slot="close" variant="outline" isDisabled={clearPending}>取消</Button>
                <Button variant="danger" isDisabled={clearPending} onPress={() => void clearCaptures()}>{clearPending ? "正在清空…" : "确认清空"}</Button>
              </AlertDialog.Footer>
            </AlertDialog.Dialog>
          </AlertDialog.Container>
        </AlertDialog.Backdrop>
      </AlertDialog>
    </section>
  );
}
