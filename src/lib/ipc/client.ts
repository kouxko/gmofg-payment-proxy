"use client";

/**
 * 前端访问 Rust 的唯一低层入口。
 *
 * 可以把这个文件理解成一座“翻译桥”：Tauri/Specta 生成的命令会返回
 * `ok/error` 联合类型，而页面更适合使用普通的 Promise。这里负责把 Rust 的
 * 成功值解包，把结构化错误抛给上层，并管理长连接事件 Channel 的订阅和释放。
 * 页面不应绕过这里直接调用 Tauri，也不应在这里实现任何代理业务规则。
 */

import { Channel } from "@tauri-apps/api/core";
import type {
  AppBootstrapViewModel,
  AppErrorViewModel,
  SubscriptionAckViewModel,
  UiEventEnvelope,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";

type GeneratedResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: AppErrorViewModel };

export async function callCommand<T>(
  result: Promise<GeneratedResult<T>>,
): Promise<T> {
  const settled = await result;
  if (settled.status === "error") {
    throw settled.error;
  }
  return settled.data;
}

/** 取得应用启动快照：产品名称、代理状态、证书、设置和事件游标都由 Rust 给出。 */
export function appBootstrap(): Promise<AppBootstrapViewModel> {
  return callCommand(commands.appBootstrap());
}

export async function subscribeToAppEvents(
  afterEventId: number,
  onEvent: (event: UiEventEnvelope) => void,
): Promise<{
  ack: SubscriptionAckViewModel;
  unsubscribe: () => Promise<void>;
}> {
  // Channel 是 Rust 向 WebView 推送有序事件的通道。afterEventId 用来告诉 Rust
  // 前端已经看到哪里，避免重连后把同一批事件重复处理。
  const channel = new Channel<UiEventEnvelope>();
  channel.onmessage = onEvent;
  const ack = await callCommand(
    commands.appSubscribeEvents(afterEventId, channel),
  );
  let active = true;
  return {
    ack,
    unsubscribe: async () => {
      // 先在浏览器侧停止接收，再通知 Rust 释放订阅，防止页面卸载后仍回调旧组件。
      if (!active) return;
      active = false;
      channel.onmessage = () => undefined;
      await callCommand(commands.appUnsubscribeEvents(ack.subscription_id));
    },
  };
}

export function errorMessage(error: unknown): string {
  const appError = appErrorViewModel(error);
  if (appError) {
    const details = Array.from(new Set(Object.values(appError.field_errors).flat()))
      .filter((message) => message.trim().length > 0);
    return details.length > 0
      ? `${appError.message}：${details.join("；")}`
      : appError.message;
  }
  return "无法连接应用核心，请确认桌面应用已完成初始化。";
}

/**
 * 判断未知异常是不是 Rust 定义的 AppErrorViewModel。
 *
 * 不在前端猜测错误码或业务原因，只做最小形状检查；识别成功后，页面原样显示
 * Rust 已经准备好的中文 message 和 field_errors。
 */
export function appErrorViewModel(
  error: unknown,
): AppErrorViewModel | undefined {
  if (
    typeof error !== "object" ||
    error === null ||
    !("message" in error) ||
    typeof (error as AppErrorViewModel).message !== "string" ||
    !("field_errors" in error) ||
    typeof (error as AppErrorViewModel).field_errors !== "object"
  ) {
    return undefined;
  }
  return error as AppErrorViewModel;
}
