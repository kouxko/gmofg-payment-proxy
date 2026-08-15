"use client";

/** 实时抓包页面容器：仅管理 Rust 查询、游标、选择和详情生命周期。 */
import { useEffect, useMemo, useState } from "react";
import { Tabs, toast } from "@heroui/react";
import type {
  CaptureDetailViewModel,
  CapturePageViewModel,
  CaptureQuery,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { CaptureDetailPanel } from "./capture-detail-panel";
import { CaptureListPanel } from "./capture-list-panel";
import { SocketCaptureView } from "./socket-capture-view";

export const defaultCaptureQuery: CaptureQuery = {
  keyword: null,
  terminal_ip: null,
  channel: null,
  stage: null,
  result: null,
  rule_id: null,
  after_event_id: null,
  sort: "occurred_at",
  direction: "desc",
  page: { page: 1, page_size: 50 },
};

export const captureDetailTabLabels = {
  overview: "概览",
  request: "请求",
  response: "响应",
} as const;

export function ruleEditorHref(sessionId: string): string {
  return `/rules?sessionId=${encodeURIComponent(sessionId)}`;
}

export function resumeCaptureQuery(query: CaptureQuery): CaptureQuery {
  return { ...query, after_event_id: null, page: { ...query.page, page: 1 } };
}

export function CaptureView({
  initialPage,
}: {
  initialPage?: CapturePageViewModel;
}) {
  const [mode, setMode] = useState<"http" | "socket">("http");
  return (
    <div className="flex h-full min-h-0 flex-col">
      <Tabs
        className="min-h-0 flex-1"
        selectedKey={mode}
        onSelectionChange={(key) => setMode(key as "http" | "socket")}
      >
        <Tabs.ListContainer className="border-b border-[var(--telemetry-line)] px-5 pt-3">
          <Tabs.List aria-label="抓包类型">
            <Tabs.Tab id="http">
              HTTP 抓包
              <Tabs.Indicator />
            </Tabs.Tab>
            <Tabs.Tab id="socket">
              Socket 抓包
              <Tabs.Indicator />
            </Tabs.Tab>
          </Tabs.List>
        </Tabs.ListContainer>
        <Tabs.Panel id={mode} className="h-full min-h-0">
          {mode === "http" ? (
            <HttpCaptureView initialPage={initialPage} />
          ) : (
            <SocketCaptureView />
          )}
        </Tabs.Panel>
      </Tabs>
    </div>
  );
}

/** HTTP 抓包保持原有查询与详情行为；只有选中 HTTP 页签时才挂载。 */
function HttpCaptureView({ initialPage }: { initialPage?: CapturePageViewModel }) {
  const { navigate } = useWorkspaceNavigation();
  const { bootstrap } = useBootstrap();
  const [paused, setPaused] = useState(false);
  const [clearPending, setClearPending] = useState(false);
  const [selectedEventId, setSelectedEventId] = useState<number>();
  const [query, setQuery] = useState(defaultCaptureQuery);
  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const page = useIpcQuery<CapturePageViewModel>(
    `capture-query:${queryKey}`,
    () => callCommand(commands.captureQuery(query)),
    initialPage,
  );
  useAppEventRefresh(
    ["capture_rows_added", "snapshot_required"],
    page.refresh,
    { paused },
  );

  useEffect(() => {
    if (!page.data?.snapshot_required || query.after_event_id == null) return;
    const task = window.setTimeout(
      () => setQuery((current) => resumeCaptureQuery(current)),
      0,
    );
    return () => window.clearTimeout(task);
  }, [page.data?.snapshot_required, query.after_event_id]);

  const selected = page.data?.rows.find(
    (row) => row.event_id === selectedEventId,
  );
  useEffect(() => {
    if (selectedEventId == null || !page.data || selected) return;
    const task = window.setTimeout(() => setSelectedEventId(undefined), 0);
    return () => window.clearTimeout(task);
  }, [page.data, selected, selectedEventId]);

  const selectedId = selected?.session_id;
  const detail = useIpcQuery<CaptureDetailViewModel>(
    `capture-detail:${selectedId ?? "none"}`,
    () =>
      callCommand(
        commands.captureGetDetail(
          selected!.session_id,
          selected!.runtime_epoch,
        ),
      ),
    undefined,
    { enabled: Boolean(selected) },
  );
  useAppEventRefresh(["session_updated"], detail.refresh, {
    paused: !selectedId,
    entityId: selectedId,
  });
  const requestHeaderCount = Object.values(
    detail.data?.request.headers ?? {},
  ).reduce((count, values) => count + values.length, 0);
  const responseHeaderCount = Object.values(
    detail.data?.response?.headers ?? {},
  ).reduce((count, values) => count + values.length, 0);

  async function clearCurrentView() {
    if (!page.data || clearPending) return;
    setClearPending(true);
    try {
      await callCommand(commands.captureClearView(page.data.event_cursor));
      setSelectedEventId(undefined);
      detail.invalidate();
      await page.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setClearPending(false);
    }
  }

  function togglePaused() {
    if (!paused) {
      setPaused(true);
      return;
    }
    setPaused(false);
    setQuery((current) => resumeCaptureQuery(current));
  }

  function selectEvent(eventId: number) {
    setSelectedEventId(eventId);
  }

  return (
    <section
      aria-label="实时抓包工作区"
      data-layout="list-only"
      className="grid h-full min-h-0 grid-cols-1 grid-rows-1"
    >
      <CaptureListPanel
        paused={paused}
        clearPending={clearPending}
        query={query}
        setQuery={setQuery}
        page={page}
        channels={bootstrap?.channel_catalog ?? []}
        selectedEventId={selectedEventId}
        onTogglePaused={togglePaused}
        onClear={() => void clearCurrentView()}
        onSelectEvent={selectEvent}
      />
      <CaptureDetailPanel
        selected={selected}
        detail={detail}
        requestHeaderCount={requestHeaderCount}
        responseHeaderCount={responseHeaderCount}
        onClose={() => {
          setSelectedEventId(undefined);
          detail.invalidate();
        }}
        onNavigate={navigate}
        onCreateRule={() => selectedId && navigate(ruleEditorHref(selectedId))}
      />
    </section>
  );
}
