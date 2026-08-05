"use client";

/**
 * 会话历史页面的状态与 IPC 编排。
 *
 * 筛选、表格、详情和确认弹窗分别由子组件渲染。本文件只维护页面级状态，
 * 并确保完整 Payload 仅在用户确实打开详情时加载。
 */

import { useMemo, useState } from "react";
import { toast } from "@heroui/react";
import type {
  SessionDetailViewModel,
  SessionPageViewModel,
  SessionQuery,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { toneColor } from "@/lib/format";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { SessionActions } from "./session-actions";
import { SessionDetailPanel } from "./session-detail-panel";
import { SessionsListPanel } from "./sessions-list-panel";
import { defaultSessionQuery } from "./session-config";

export {
  defaultSessionQuery,
  sessionDetailTabLabels,
  sessionFilterDateText,
  sessionFilterDateValue,
} from "./session-config";

export function SessionsView() {
  const { bootstrap } = useBootstrap();
  const [selectedId, setSelectedId] = useState<string>();
  const [query, setQuery] = useState<SessionQuery>(defaultSessionQuery);
  const [detailOpen, setDetailOpen] = useState(false);
  const [detailRequested, setDetailRequested] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportPending, setExportPending] = useState(false);
  const [clearDialogOpen, setClearDialogOpen] = useState(false);
  const [clearPending, setClearPending] = useState(false);

  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const page = useIpcQuery<SessionPageViewModel>(
    `session-query:${queryKey}`,
    () => callCommand(commands.sessionQuery(query)),
  );
  useAppEventRefresh(["session_updated", "snapshot_required"], page.refresh);

  const detail = useIpcQuery<SessionDetailViewModel>(
    `session-detail:${detailRequested ? (selectedId ?? "none") : "closed"}`,
    () => callCommand(commands.sessionGet(selectedId!)),
    undefined,
    { enabled: Boolean(selectedId && detailRequested) },
  );
  useAppEventRefresh(["session_updated"], detail.refresh, {
    paused: !selectedId || !detailRequested,
    entityId: selectedId,
  });

  const selected = page.data?.items.find(
    (item) => item.session_id === selectedId,
  );

  function selectSession(id: string) {
    setSelectedId(id);
    detail.invalidate();
    setDetailRequested(window.matchMedia("(min-width: 1281px)").matches);
  }

  function closeDetail() {
    setDetailOpen(false);
    setSelectedId(undefined);
    setDetailRequested(false);
    detail.invalidate();
  }

  async function exportSelected() {
    if (!selectedId || exportPending) return;

    setExportPending(true);
    try {
      const result = await callCommand(
        commands.sessionExport(selectedId, true),
      );
      toast(result.message, { variant: toneColor(result.ui_tone) });
      setExportDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setExportPending(false);
    }
  }

  async function clearSessions() {
    if (clearPending) return;

    setClearPending(true);
    try {
      const result = await callCommand(commands.sessionClear(true));
      toast(result.message, { variant: toneColor(result.ui_tone) });
      closeDetail();
      await page.refresh();
      setClearDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setClearPending(false);
    }
  }

  return (
    <section className="grid h-full grid-cols-[minmax(0,1fr)_380px] max-[1280px]:grid-cols-1">
      <div className="min-w-0 space-y-4 overflow-auto p-5">
        <SessionsListPanel
          query={query}
          setQuery={setQuery}
          page={page}
          channels={bootstrap?.channel_catalog ?? []}
          selectedId={selectedId}
          onSelect={selectSession}
        />
        <SessionActions
          selected={selected}
          detail={detail}
          detailOpen={detailOpen}
          exportDialogOpen={exportDialogOpen}
          exportPending={exportPending}
          clearDialogOpen={clearDialogOpen}
          clearPending={clearPending}
          onDetailOpenChange={(open) => {
            setDetailOpen(open);
            setDetailRequested(open);
            if (!open) detail.invalidate();
          }}
          onExportDialogOpenChange={setExportDialogOpen}
          onExport={() => void exportSelected()}
          onClearDialogOpenChange={setClearDialogOpen}
          onClear={() => void clearSessions()}
        />
      </div>
      <SessionDetailPanel
        selected={selected}
        detail={detail}
        onClose={closeDetail}
      />
    </section>
  );
}
