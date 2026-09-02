import type {
  ProxyListener,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";

interface CurrentWorkspaceListeners {
  listeners: ProxyListener[];
  loading: boolean;
  error?: string;
}

/**
 * Android Profile 只保存稳定的 ListenerId。当前工作区和入口列表仍由 Rust 提供，
 * 页面不能缓存桌面 IP、ADB 端口或其他运行态地址。
 */
export function useCurrentWorkspaceListeners(): CurrentWorkspaceListeners {
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "android-route-workspaces",
    () => callCommand(commands.workspaceList()),
    [],
  );
  const currentId = workspaces.data?.find((workspace) => workspace.selected)?.id
    ?? workspaces.data?.[0]?.id;
  const workspace = useIpcQuery<ProxyWorkspace>(
    `android-route-workspace:${currentId ?? "none"}`,
    () => callCommand(commands.workspaceGet(currentId!)),
    undefined,
    { enabled: Boolean(currentId) },
  );

  return {
    listeners: workspace.data?.listeners ?? [],
    loading: workspaces.isLoading || workspace.isLoading,
    error: [workspaces.error, workspace.error].filter(Boolean).join("；") || undefined,
  };
}
