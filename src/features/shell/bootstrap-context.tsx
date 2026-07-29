"use client";

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
                  channel.kind === channelStatus.kind ? channelStatus : channel,
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
      if (event.payload.type === "snapshot_required") {
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
  options?: { paused?: boolean },
) {
  const { subscribe } = useBootstrap();
  const eventTypesKey = eventTypes.join("|");
  useEffect(() => {
    if (options?.paused) return;
    const acceptedTypes = new Set(eventTypesKey.split("|"));
    let refreshPending = false;
    return subscribe((event) => {
      if (!acceptedTypes.has(event.payload.type) || refreshPending) return;
      refreshPending = true;
      void refresh().finally(() => {
        refreshPending = false;
      });
    });
  }, [eventTypesKey, options?.paused, refresh, subscribe]);
}

export function useBootstrap() {
  const value = useContext(BootstrapContext);
  if (!value) {
    throw new Error("useBootstrap 必须在 BootstrapProvider 内使用");
  }
  return value;
}
