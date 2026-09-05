"use client";

import { createContext, useContext, useMemo } from "react";
import type { WorkspaceSummaryViewModel } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useAppEventRefresh } from "./bootstrap-context";

interface SelectedWorkspaceState {
  summaries: WorkspaceSummaryViewModel[];
  selected?: WorkspaceSummaryViewModel;
  isLoading: boolean;
  error?: string;
  refresh: () => Promise<void>;
}

const SelectedWorkspaceContext = createContext<SelectedWorkspaceState | null>(null);

export function SelectedWorkspaceProvider({ children }: Readonly<{ children: React.ReactNode }>) {
  const query = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "selected-workspace",
    () => callCommand(commands.workspaceList()),
  );
  useAppEventRefresh(["workspace_changed", "snapshot_required"], query.refresh);
  const value = useMemo<SelectedWorkspaceState>(() => ({
    ...selectedWorkspaceState(query.data, query.error),
    isLoading: query.isLoading,
    refresh: query.refresh,
  }), [query.data, query.error, query.isLoading, query.refresh]);
  return (
    <SelectedWorkspaceContext.Provider value={value}>
      {children}
    </SelectedWorkspaceContext.Provider>
  );
}

function selectedWorkspaceState(
  summaries: WorkspaceSummaryViewModel[] | undefined,
  queryError: string | undefined,
): Pick<SelectedWorkspaceState, "summaries" | "selected" | "error"> {
  const selected = summaries?.filter((workspace) => workspace.selected) ?? [];
  return {
    summaries: summaries ?? [],
    selected: selected.length === 1 ? selected[0] : undefined,
    error: queryError ?? (summaries !== undefined && selected.length !== 1
      ? "当前 Workspace 选择状态无效。"
      : undefined),
  };
}

export function useSelectedWorkspace(): SelectedWorkspaceState {
  const value = useContext(SelectedWorkspaceContext);
  if (!value) throw new Error("useSelectedWorkspace 必须在 SelectedWorkspaceProvider 内使用");
  return value;
}
