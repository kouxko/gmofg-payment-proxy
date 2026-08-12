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
  SessionListViewModel,
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
  const [clearDialogOpen, setClearDialogOpen] = useState(false);
  const [clearPending, setClearPending] = useState(false);

  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const list = useIpcQuery<SessionListViewModel>(
    `session-query:${queryKey}`,
    () => callCommand(commands.sessionQuery(query)),
  );
  useAppEventRefresh(["session_updated", "snapshot_required"], list.refresh);

  const detail = useIpcQuery<SessionDetailViewModel>(
    `session-detail:${detailOpen ? (selectedId ?? "none") : "closed"}`,
    () => callCommand(commands.sessionGet(selectedId!)),
    undefined,
    { enabled: Boolean(selectedId && detailOpen) },
  );
  useAppEventRefresh(["session_updated"], detail.refresh, {
    paused: !selectedId || !detailOpen,
    entityId: selectedId,
  });

  const selected = list.data?.items.find(
    (item) => item.session_id === selectedId,
  );

  function selectSession(id: string) {
    setSelectedId(id);
    detail.invalidate();
    setDetailOpen(true);
  }

  function closeDetail() {
    setDetailOpen(false);
    setSelectedId(undefined);
    detail.invalidate();
  }

  async function clearSessions() {
    if (clearPending) return;

    setClearPending(true);
    try {
      const result = await callCommand(commands.sessionClear(true));
      toast(result.message, { variant: toneColor(result.ui_tone) });
      closeDetail();
      await list.refresh();
      setClearDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setClearPending(false);
    }
  }

  return (
    <section className="h-full">
      <div className="min-w-0 space-y-4 overflow-auto p-5">
        <SessionsListPanel
          query={query}
          setQuery={setQuery}
          list={list}
          channels={bootstrap?.channel_catalog ?? []}
          selectedId={selectedId}
          onSelect={selectSession}
        />
        <SessionActions
          selected={selected}
          detail={detail}
          detailOpen={detailOpen}
          clearDialogOpen={clearDialogOpen}
          clearPending={clearPending}
          onDetailOpenChange={(open) => {
            setDetailOpen(open);
            if (!open) detail.invalidate();
          }}
          onClearDialogOpenChange={setClearDialogOpen}
          onClear={() => void clearSessions()}
        />
      </div>
    </section>
  );
}
