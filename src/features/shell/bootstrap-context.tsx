"use client";

/**
 * 前端的全局 Rust 快照与事件订阅协调器。
 *
 * 启动时先通过 app_bootstrap 取得完整快照，再从快照中的 event_cursor 开始订阅
 * Rust Channel。常见状态事件会就地更新顶部栏；需要一致性重建时，再合并触发
 * 一次快照刷新。前端不持久化这些数据，也不推导代理业务状态。
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type {
  AppBootstrapViewModel,
  ProxyStatusViewModel,
  UiEventEnvelope,
} from "@/generated/rust-types";
import {
  appBootstrap,
  errorMessage,
  subscribeToAppEvents,
} from "@/lib/ipc/client";

type BootstrapState = {
  bootstrap?: AppBootstrapViewModel;
  proxy?: ProxyStatusViewModel;
  isLoading: boolean;
  error?: string;
  refresh: () => Promise<void>;
  subscribe: (listener: (event: UiEventEnvelope) => void) => () => void;
};

const BootstrapContext = createContext<BootstrapState | null>(null);

export function BootstrapProvider({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  const [bootstrap, setBootstrap] = useState<AppBootstrapViewModel>();
  const [proxy, setProxy] = useState<ProxyStatusViewModel>();
  const [error, setError] = useState<string>();
  const [isLoading, setIsLoading] = useState(true);
  const listeners = useRef(new Set<(event: UiEventEnvelope) => void>());
  const refreshTimer = useRef<number | undefined>(undefined);
  const refreshGeneration = useRef(0);

  const refresh = useCallback(async () => {
    // generation 解决并发刷新竞态：只有最后一次请求可以替换全局快照。
    const generation = refreshGeneration.current + 1;
    refreshGeneration.current = generation;
    setIsLoading(true);
    setError(undefined);
    try {
      const next = await appBootstrap();
      if (generation !== refreshGeneration.current) return;
      setBootstrap(next);
      setProxy(next.proxy);
    } catch (reason) {
      if (generation === refreshGeneration.current) {
        setError(errorMessage(reason));
      }
    } finally {
      if (generation === refreshGeneration.current) setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    // 推迟到当前 React 提交之后再首次加载，避免在 effect 同步阶段级联 setState。
    const task = window.setTimeout(() => void refresh(), 0);
    return () => window.clearTimeout(task);
  }, [refresh]);

  const eventCursor = bootstrap?.event_cursor;
  const subscribe = useCallback(
    (listener: (event: UiEventEnvelope) => void) => {
      listeners.current.add(listener);
      return () => listeners.current.delete(listener);
    },
    [],
  );

  useEffect(() => {
    if (eventCursor == null) return;
    let active = true;
    let unsubscribe: (() => Promise<void>) | undefined;
    const scheduleSnapshotRefresh = () => {
      // 多个事件常在同一批到达。50ms 合并窗口避免每条事件都发一次全量查询。
      if (refreshTimer.current != null) return;
      refreshTimer.current = window.setTimeout(() => {
        refreshTimer.current = undefined;
        if (active) void refresh();
      }, 50);
    };
    const handleEvent = (event: UiEventEnvelope) => {
      if (!active) return;
      refreshGeneration.current += 1;
      setIsLoading(false);
      listeners.current.forEach((listener) => listener(event));
      if (event.payload.type === "runtime_status_changed") {
        // 高频且体积小的状态直接补丁更新，顶部栏可以立即响应。
        const status = event.payload.data;
        setProxy(status);
        setBootstrap((current) =>
          current ? { ...current, proxy: status } : current,
        );
      }
      if (event.payload.type === "channel_status_changed") {
        const channelStatus = event.payload.data;
        setProxy((current) =>
          current
            ? {
                ...current,
                channels: current.channels.map((channel) =>
                  channel.id === channelStatus.id ? channelStatus : channel,
                ),
              }
            : current,
        );
      }
      if (event.payload.type === "certificate_status_changed") {
        const certificate = event.payload.data;
        setBootstrap((current) =>
          current
            ? { ...current, certificate }
            : current,
        );
      }
      if (event.payload.type === "settings_changed") {
        const settings = event.payload.data;
        setBootstrap((current) =>
          current ? { ...current, settings } : current,
        );
      }
      if (event.payload.type === "snapshot_required") {
        // Rust 明确要求重取时，以完整快照为最终事实来源。
        scheduleSnapshotRefresh();
      }
      if (event.payload.type === "operation_failed") {
        setError(event.payload.data.message);
      }
      if (event.payload.type === "resource_warning") {
        setError(event.payload.data.message);
      }
      if (
        [
          "channel_status_changed",
          "session_updated",
          "breakpoint_queued",
          "breakpoint_resolved",
          "certificate_status_changed",
          "settings_changed",
        ].includes(event.payload.type)
      ) {
        scheduleSnapshotRefresh();
      }
    };
    void subscribeToAppEvents(eventCursor, handleEvent)
      .then((subscription) => {
        if (!active) {
          void subscription.unsubscribe();
          return;
        }
        unsubscribe = subscription.unsubscribe;
        if (subscription.ack.snapshot_required) scheduleSnapshotRefresh();
      })
      .catch((reason) => active && setError(errorMessage(reason)));
    return () => {
      active = false;
      if (unsubscribe) void unsubscribe();
      if (refreshTimer.current != null) {
        window.clearTimeout(refreshTimer.current);
        refreshTimer.current = undefined;
      }
    };
  }, [eventCursor, refresh]);

  const value = useMemo(
    () => ({ bootstrap, proxy, isLoading, error, refresh, subscribe }),
    [bootstrap, proxy, isLoading, error, refresh, subscribe],
  );

  return (
    <BootstrapContext.Provider value={value}>
      {children}
    </BootstrapContext.Provider>
  );
}

export function useAppEventRefresh(
  eventTypes: readonly UiEventEnvelope["payload"]["type"][],
  refresh: () => Promise<void>,
  options?: { paused?: boolean; entityId?: string },
) {
  /**
   * 页面级事件刷新器：只监听调用方关心的事件类型，可按 entityId 精确过滤。
   * 刷新执行期间再次收到事件不会并发发请求，而是记为 refreshAgain，前一次完成
   * 后再补一次，既不丢最终状态也不制造请求风暴。
   */
  const { subscribe } = useBootstrap();
  const eventTypesKey = eventTypes.join("|");
  useEffect(() => {
    if (options?.paused) return;
    const acceptedTypes = new Set(eventTypesKey.split("|"));
    let refreshPending = false;
    let refreshAgain = false;
    let active = true;
    const drainRefreshes = async () => {
      refreshPending = true;
      do {
        refreshAgain = false;
        await refresh();
      } while (active && refreshAgain);
      refreshPending = false;
    };
    const unsubscribe = subscribe((event) => {
      if (
        !acceptedTypes.has(event.payload.type) ||
        (options?.entityId != null && event.entity_id !== options.entityId)
      ) {
        return;
      }
      if (refreshPending) {
        refreshAgain = true;
        return;
      }
      void drainRefreshes();
    });
    return () => {
      active = false;
      unsubscribe();
    };
  }, [eventTypesKey, options?.entityId, options?.paused, refresh, subscribe]);
}

export function useBootstrap() {
  const value = useContext(BootstrapContext);
  if (!value) {
    throw new Error("useBootstrap 必须在 BootstrapProvider 内使用");
  }
  return value;
}
