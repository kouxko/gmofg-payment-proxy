"use client";

/**
 * 统一运行记录入口。
 *
 * HTTP 与 Socket 都由 Rust 的 ExchangeObservationStore 提供连接级时间线，
 * 页面不再挂载旧 HTTP Session 列表，避免同一笔 HTTP 交易重复显示两次。
 */
import { ExchangeObservationView } from "./exchange-observation-view";

export function CaptureView() {
  return <ExchangeObservationView />;
}
