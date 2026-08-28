"use client";

import { useCallback } from "react";
import type { ProxyWorkspace, RuleDefinition_Serialize, WorkspaceSummaryViewModel } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useWorkspaceQueryInvalidation } from "@/features/shell/workspace-navigation";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";

export function useRuleDefinitionSource() {
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "rule-workspaces",
    () => callCommand(commands.workspaceList()),
  );
  const workspaceId = workspaces.data?.find((workspace) => workspace.selected)?.id;
  const workspace = useIpcQuery<ProxyWorkspace>(
    `rule-workspace:${workspaceId ?? "none"}`,
    () => callCommand(commands.workspaceGet(workspaceId!)),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const rules = useIpcQuery<RuleDefinition_Serialize[]>(
    `rule-definition-list:${workspaceId ?? "none"}`,
    () => callCommand(commands.ruleDefinitionList()),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const refreshWorkspaces = workspaces.refresh;
  const refreshWorkspace = workspace.refresh;
  const refreshRules = rules.refresh;
  const refresh = useCallback(async () => {
    await Promise.all([refreshWorkspaces(), refreshWorkspace(), refreshRules()]);
  }, [refreshRules, refreshWorkspace, refreshWorkspaces]);

  useWorkspaceQueryInvalidation({
    workspaceId,
    collection: [workspaces],
    current: [workspace, rules],
  });

  return {
    workspaceId,
    listeners: workspace.data?.listeners ?? [],
    rules: rules.data ?? [],
    error: workspaces.error ?? workspace.error ?? rules.error,
    isLoading: workspaces.isLoading || workspace.isLoading || rules.isLoading,
    refresh,
  };
}
