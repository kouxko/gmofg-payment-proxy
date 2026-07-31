"use client";

/**
 * 面向 Tauri Command 的轻量查询 Hook。
 *
 * 它只管理“加载中/成功/失败/刷新/失效”这些显示状态，不负责筛选、分页或业务
 * 计算。真正的数据仍由 Rust 查询并返回。generation 计数用于丢弃过期响应：
 * 用户快速切换条件时，较早发出的慢请求不能覆盖较新的页面结果。
 */

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
    // load 通常是组件内联函数。保存最新引用，可以让 refresh 保持稳定，避免
    // 仅因函数身份变化而产生重复请求。
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
      // 只有当前代请求有权写回；更旧请求即使后来成功，也必须被静默丢弃。
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
    // queryKey 是查询条件的稳定序列化值。条件改变时自动加载一次；组件销毁或
    // 条件再次变化会推进 generation，从而让旧 Promise 失效。
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
