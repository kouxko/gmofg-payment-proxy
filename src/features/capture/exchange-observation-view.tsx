"use client";

import { useMemo, useState } from "react";
import { Alert, AlertDialog, Button, Spinner, toast } from "@heroui/react";
import type {
  ExchangeObservationPage,
  ExchangeObservationRecord,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { ExchangeObservationDetail } from "./exchange-observation-detail";
import { ExchangeObservationList } from "./exchange-observation-list";
import { defaultExchangeObservationQuery } from "./exchange-observation-model";

function selectedWorkspace(value: WorkspaceSummaryViewModel[] | undefined) {
  const selected = value?.filter((workspace) => workspace.selected) ?? [];
  return selected.length === 1 ? selected[0] : undefined;
}

export function ExchangeObservationView() {
  const { navigate } = useWorkspaceNavigation();
  const [pageNumber, setPageNumber] = useState(1);
  const [selectedId, setSelectedId] = useState<string>();
  const [clearOpen, setClearOpen] = useState(false);
  const [clearPending, setClearPending] = useState(false);
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "exchange-observation-workspaces",
    () => callCommand(commands.workspaceList()),
  );
  const workspace = selectedWorkspace(workspaces.data);
  const query = useMemo(
    () => workspace ? defaultExchangeObservationQuery(workspace.id, pageNumber) : undefined,
    [pageNumber, workspace],
  );
  const page = useIpcQuery<ExchangeObservationPage>(
    `exchange-observation-query:${query ? JSON.stringify(query) : "disabled"}`,
    () => callCommand(commands.exchangeObservationQuery(query!)),
    undefined,
    { enabled: Boolean(query) },
  );
  useAppEventRefresh(["workspace_changed"], workspaces.refresh);
  useAppEventRefresh(
    ["exchange_observation_changed", "snapshot_required", "workspace_changed"],
    page.refresh,
    { paused: !query },
  );

  const selected = page.data?.rows.find((record) => record.exchange_id === selectedId);
  const detail = useIpcQuery<ExchangeObservationRecord>(
    `exchange-observation-detail:${selected?.exchange_id ?? "none"}`,
    () => callCommand(commands.exchangeObservationGet(selected!.exchange_id)),
    undefined,
    { enabled: Boolean(selected) },
  );
  useAppEventRefresh(["exchange_observation_changed"], detail.refresh, {
    paused: !selectedId,
    entityId: selectedId,
  });

  async function clearRecords() {
    if (!workspace || clearPending) return;
    const targetWorkspaceId = workspace.id;
    setClearPending(true);
    try {
      const result = await callCommand(commands.exchangeObservationClear(targetWorkspaceId, true));
      if (!result.success) throw new Error(result.message);
      setSelectedId(undefined);
      detail.invalidate();
      setPageNumber(1);
      await page.refresh();
      setClearOpen(false);
      toast(result.message, { variant: "success" });
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setClearPending(false);
    }
  }

  if (workspaces.isLoading) return <div className="grid h-full place-items-center"><Spinner aria-label="正在读取当前工作区" /></div>;
  if (workspaces.error || !workspace) return <div className="grid h-full place-items-center p-5"><Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>无法确定唯一的当前工作区</Alert.Title><Alert.Description>{workspaces.error ?? "请选择一个工作区后再查看 Exchange。"}</Alert.Description></Alert.Content><Button size="sm" variant="outline" onPress={() => void workspaces.refresh()}>重试</Button></Alert></div>;

  return <section aria-label="统一运行记录工作区" className="grid h-full min-h-0 grid-cols-1 grid-rows-1">
    <ExchangeObservationList page={page.data} error={page.error} loading={page.isLoading} selectedId={selectedId} onSelect={(record) => setSelectedId(record.exchange_id)} onPage={setPageNumber} onRetry={() => void page.refresh()} onClear={() => setClearOpen(true)} />
    <ExchangeObservationDetail selected={selected} detail={detail.data} error={detail.error} loading={detail.isLoading} onClose={() => { const focusId = selectedId; setSelectedId(undefined); detail.invalidate(); if (focusId) requestAnimationFrame(() => document.getElementById(`exchange-observation-row-${focusId}`)?.focus()); }} onRetry={() => void detail.refresh()} onCreateMockDraft={(exchangeId, responseEventIndex) => navigate(`/rules?exchangeId=${encodeURIComponent(exchangeId)}&responseEvent=${responseEventIndex}`)} />
    <AlertDialog isOpen={clearOpen} onOpenChange={(open) => { if (!clearPending) setClearOpen(open); }}>
      <Button className="hidden" aria-hidden="true">打开清空确认</Button>
      <AlertDialog.Backdrop><AlertDialog.Container><AlertDialog.Dialog>
        <AlertDialog.Header><AlertDialog.Heading>清空当前工作区的运行记录？</AlertDialog.Heading></AlertDialog.Header>
        <AlertDialog.Body>记录只存在于内存；清空不会停止入口，也不会删除 Workspace、Listener 或 Rules 配置。</AlertDialog.Body>
        <AlertDialog.Footer><Button slot="close" variant="outline" isDisabled={clearPending}>取消</Button><Button variant="danger" isDisabled={clearPending} onPress={() => void clearRecords()}>{clearPending ? "正在清空…" : "确认清空"}</Button></AlertDialog.Footer>
      </AlertDialog.Dialog></AlertDialog.Container></AlertDialog.Backdrop>
    </AlertDialog>
  </section>;
}
