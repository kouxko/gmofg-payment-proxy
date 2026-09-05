import type {
  ProxyListener,
  ProxyWorkspace,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useSelectedWorkspace } from "@/features/shell/selected-workspace-context";

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
  const selectedWorkspace = useSelectedWorkspace();
  const currentId = selectedWorkspace.selected?.id;
  const workspace = useIpcQuery<ProxyWorkspace>(
    `android-route-workspace:${currentId ?? "none"}`,
    () => callCommand(commands.workspaceGet(currentId!)),
    undefined,
    { enabled: Boolean(currentId) },
  );

  return {
    listeners: workspace.data?.listeners ?? [],
    loading: selectedWorkspace.isLoading || workspace.isLoading,
    error: [selectedWorkspace.error, workspace.error].filter(Boolean).join("；") || undefined,
  };
}
