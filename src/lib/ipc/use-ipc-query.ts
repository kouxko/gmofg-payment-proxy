"use client";

import {
  useCallback,
  useEffect,
  useEffectEvent,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { errorMessage } from "@/lib/ipc/client";

interface IpcQueryOptions {
  enabled?: boolean;
  clearOnDisable?: boolean;
}

export function useIpcQuery<T>(
  queryKey: string,
  load: () => Promise<T>,
  initialData?: T,
  options: IpcQueryOptions = {},
) {
  const { enabled = true, clearOnDisable = true } = options;
  const [data, setData] = useState<T | undefined>(initialData);
  const [error, setError] = useState<string>();
  const [isLoading, setIsLoading] = useState(initialData === undefined);
  const requestGeneration = useRef(0);
  const loadRef = useRef(load);
  useLayoutEffect(() => {
    loadRef.current = load;
  }, [load]);
  const loadLatest = useEffectEvent(load);

  const invalidate = useCallback(
    (clearData = true) => {
      requestGeneration.current += 1;
      setIsLoading(false);
      setError(undefined);
      if (clearData) setData(undefined);
    },
    [],
  );

  const refresh = useCallback(async () => {
    if (!enabled) return;
    const generation = requestGeneration.current + 1;
    requestGeneration.current = generation;
    setIsLoading(true);
    setError(undefined);
    try {
      const next = await loadRef.current();
      if (generation === requestGeneration.current) setData(next);
    } catch (reason) {
      if (generation === requestGeneration.current) {
        setError(errorMessage(reason));
      }
    } finally {
      if (generation === requestGeneration.current) setIsLoading(false);
    }
  }, [enabled]);

  useEffect(() => {
    const generation = requestGeneration.current + 1;
    requestGeneration.current = generation;
    Promise.resolve()
      .then(() => {
        if (!enabled) {
          setIsLoading(false);
          setError(undefined);
          if (clearOnDisable) setData(undefined);
          return undefined;
        }
        if (generation === requestGeneration.current) {
          setIsLoading(true);
          setError(undefined);
        }
        return loadLatest();
      })
      .then((next) => {
        if (
          enabled &&
          next !== undefined &&
          generation === requestGeneration.current
        ) {
          setData(next);
        }
      })
      .catch((reason) => {
        if (enabled && generation === requestGeneration.current) {
          setError(errorMessage(reason));
        }
      })
      .finally(() => {
        if (generation === requestGeneration.current) setIsLoading(false);
      });
  }, [clearOnDisable, enabled, queryKey]);

  return { data, error, isLoading, refresh, setData, invalidate };
}
