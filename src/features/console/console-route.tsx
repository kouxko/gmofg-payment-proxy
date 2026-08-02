"use client";

/**
 * 控制台的数据装配层。
 *
 * 当前工作区及其入口概览都由 Rust Command 读取，最近抓包使用独立查询加载。
 * 页面不再读取旧的全局 ProxySupervisor 状态，避免和“入口配置”形成第二套目录。
 */

import { Alert, Button, Spinner } from "@heroui/react";
import { ConsoleView } from "@/features/console/console-view";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { callCommand } from "@/lib/ipc/client";
import { commands } from "@/generated/rust-types";
import type {
  ListenerOverviewViewModel,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { defaultCaptureQuery } from "@/features/capture/capture-view";

export function ConsoleRoute() {
  const { bootstrap } = useBootstrap();
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "console-workspaces",
    () => callCommand(commands.workspaceList()),
  );
  const workspaceId =
    workspaces.data?.find((workspace) => workspace.selected)?.id ??
    workspaces.data?.[0]?.id;
  const overview = useIpcQuery<ListenerOverviewViewModel>(
    `console-listener-overview:${workspaceId ?? "none"}`,
    () => callCommand(commands.listenerOverview(workspaceId!)),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const recentCapture = useIpcQuery(
    "console-recent-capture",
    () =>
      callCommand(
        commands.captureQuery({
          ...defaultCaptureQuery,
          page: { page: 1, page_size: 10 },
        }),
      ),
    bootstrap?.recent_capture,
  );
  useAppEventRefresh(
    // 新抓包或 Rust 要求重建快照时，仅刷新“最近事件”这一小块。
    ["capture_rows_added", "snapshot_required"],
    recentCapture.refresh,
  );
  useAppEventRefresh(["workspace_changed"], workspaces.refresh);
  useAppEventRefresh(
    ["workspace_changed", "listener_status_changed", "snapshot_required"],
    overview.refresh,
  );

  if (!overview.data) {
    const error = workspaces.error ?? overview.error;
    if (error) {
      return (
        <div className="grid h-full place-items-center p-5">
          <Alert status="danger" className="max-w-xl">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>运行监控加载失败</Alert.Title>
              <Alert.Description>{error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => {
                void workspaces.refresh();
                void overview.refresh();
              }}
            >
              重试
            </Button>
          </Alert>
        </div>
      );
    }
    return (
      <div className="grid h-full place-items-center">
        <Spinner aria-label="正在加载运行监控" />
      </div>
    );
  }
  return (
    <ConsoleView
      overview={overview.data}
      recentCapture={recentCapture.data}
      recentCaptureError={recentCapture.error}
      recentCaptureLoading={recentCapture.isLoading}
      onRecentCaptureRetry={recentCapture.refresh}
    />
  );
}
