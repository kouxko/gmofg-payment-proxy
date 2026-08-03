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
  const [isLoading, setIsLoading] = useState(
    enabled && initialData === undefined,
  );
  const [activeQueryKey, setActiveQueryKey] = useState(queryKey);
  const requestGeneration = useRef(0);
  const startedQueryKey = useRef(queryKey);
  const loadRef = useRef(load);
  useLayoutEffect(() => {
    // load 通常是组件内联函数。保存最新引用，可以让 refresh 保持稳定，避免
    // 仅因函数身份变化而产生重复请求。
    loadRef.current = load;
  }, [load]);
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
    let disposed = false;

    async function loadQuery() {
      const queryChanged = startedQueryKey.current !== queryKey;
      startedQueryKey.current = queryKey;
      if (!enabled) {
        if (disposed || generation !== requestGeneration.current) return;
        setActiveQueryKey(queryKey);
        setIsLoading(false);
        setError(undefined);
        if (clearOnDisable) setData(undefined);
        return;
      }
      if (!disposed && generation === requestGeneration.current) {
        setActiveQueryKey(queryKey);
        if (queryChanged) setData(undefined);
        setIsLoading(true);
        setError(undefined);
      }
      try {
        // 通过 ref 读取本次渲染的最新 loader。这里不能使用 useEffectEvent：
        // 部分 WebView/React 组合会把异步 effect 中的调用判定为非法副作用，
        // 查询既不报错也不写回数据，证书卡片就会永久停在“正在读取”。
        const next = await loadRef.current();
        if (
          !disposed
          && next !== undefined
          && generation === requestGeneration.current
        ) {
          setData(next);
        }
      } catch (reason) {
        if (!disposed && generation === requestGeneration.current) {
          setError(errorMessage(reason));
        }
      } finally {
        if (!disposed && generation === requestGeneration.current) {
          setIsLoading(false);
        }
      }
    }

    void loadQuery();
    return () => {
      disposed = true;
      // 无条件推进代次，同时使 effect 请求和用户主动 refresh 的请求全部失效。
      // 否则 refresh 比本 effect 更新时，卸载清理会漏掉它，仍可能写入已卸载组件。
      requestGeneration.current += 1;
    };
  }, [clearOnDisable, enabled, queryKey]);

  // queryKey 在渲染阶段已经变化，而 effect 尚未开始时，也必须立即隐藏旧数据并
  // 显示加载态。这样列表选中态可以先完成绘制，完整 Payload 再异步读取；旧会话
  // 的详情不会在新会话标题下短暂闪现。
  const isCurrentQuery = activeQueryKey === queryKey;
  return {
    data: isCurrentQuery ? data : undefined,
    error: isCurrentQuery ? error : undefined,
    isLoading: enabled && (!isCurrentQuery || isLoading),
    refresh,
    setData,
    invalidate,
  };
}
