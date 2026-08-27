"use client";

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from "react";

export type RuleCreationOption = {
  disabled: boolean;
  reason?: string;
};

export const ruleCreationReasons = {
  http: "当前 Workspace 没有 HTTP Listener；请先创建 HTTP 入口。",
  body: "当前 Workspace 没有启用协议 Body 处理的 HTTP Listener；请先为 HTTP 入口配置协议包并将 Body 处理设为协议模式。",
  socket: "当前 Workspace 没有启用报文处理的 Socket Listener；请先为 Socket 入口配置协议包并将报文处理设为脚本模式。",
} as const;

export function ruleCreationOption(
  loading: boolean,
  error: unknown,
  available: boolean,
  unavailableReason: string,
): RuleCreationOption {
  if (loading) return { disabled: true, reason: "正在读取当前 Workspace 的入口配置。" };
  if (error) return { disabled: true, reason: "入口配置读取失败，请刷新后重试。" };
  return available
    ? { disabled: false }
    : { disabled: true, reason: unavailableReason };
}

export function useRuleEditorRequestGuard(context: string) {
  const generationRef = useRef(0);
  const contextRef = useRef(context);
  useLayoutEffect(() => {
    contextRef.current = context;
  }, [context]);
  useEffect(() => () => {
    generationRef.current += 1;
  }, []);
  const begin = useCallback(() => {
    generationRef.current += 1;
    return { generation: generationRef.current, context: contextRef.current };
  }, []);
  const invalidate = useCallback(() => {
    generationRef.current += 1;
  }, []);
  const isCurrent = useCallback(
    (request: { generation: number; context: string }) =>
      request.generation === generationRef.current
      && request.context === contextRef.current,
    [],
  );
  return useMemo(
    () => ({ begin, invalidate, isCurrent }),
    [begin, invalidate, isCurrent],
  );
}
