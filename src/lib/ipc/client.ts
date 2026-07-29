"use client";

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
  const channel = new Channel<UiEventEnvelope>();
  channel.onmessage = onEvent;
  const ack = await callCommand(
    commands.appSubscribeEvents(afterEventId, channel),
  );
  let active = true;
  return {
    ack,
    unsubscribe: async () => {
      if (!active) return;
      active = false;
      channel.onmessage = () => undefined;
      await callCommand(commands.appUnsubscribeEvents(ack.subscription_id));
    },
  };
}

export function errorMessage(error: unknown): string {
  const appError = appErrorViewModel(error);
  if (appError) return appError.message;
  return "无法连接 Rust 核心，请确认桌面应用已完成初始化。";
}

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
