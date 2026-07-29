"use client";

import { Alert, Button, Spinner } from "@heroui/react";
import { ConsoleView } from "@/features/console/console-view";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { callCommand } from "@/lib/ipc/client";
import { commands } from "@/generated/rust-types";
import { defaultCaptureQuery } from "@/features/capture/capture-view";

export function ConsoleRoute() {
  const { bootstrap, proxy, refresh, error } = useBootstrap();
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
    ["capture_rows_added", "snapshot_required"],
    recentCapture.refresh,
  );
  if (!proxy) {
    if (error) {
      return (
        <div className="grid h-full place-items-center p-5">
          <Alert status="danger" className="max-w-xl">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>代理控制台加载失败</Alert.Title>
              <Alert.Description>{error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void refresh()}
            >
              重试
            </Button>
          </Alert>
        </div>
      );
    }
    return (
      <div className="grid h-full place-items-center">
        <Spinner aria-label="正在加载代理控制台" />
      </div>
    );
  }
  return (
    <ConsoleView
      status={proxy}
      recentCapture={recentCapture.data}
      recentCaptureError={recentCapture.error}
      recentCaptureLoading={recentCapture.isLoading}
      onRecentCaptureRetry={recentCapture.refresh}
      onRefresh={refresh}
    />
  );
}
