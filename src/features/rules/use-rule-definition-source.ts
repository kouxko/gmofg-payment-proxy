"use client";

import { useCallback } from "react";
import type { ProxyWorkspace, RuleDefinition_Serialize } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useWorkspaceQueryInvalidation } from "@/features/shell/workspace-navigation";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useSelectedWorkspace } from "@/features/shell/selected-workspace-context";

export function useRuleDefinitionSource() {
  const selectedWorkspace = useSelectedWorkspace();
  const workspaceId = selectedWorkspace.selected?.id;
  const refreshSelectedWorkspace = selectedWorkspace.refresh;
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
  const refreshWorkspace = workspace.refresh;
  const refreshRules = rules.refresh;
  const refresh = useCallback(async () => {
    await Promise.all([refreshSelectedWorkspace(), refreshWorkspace(), refreshRules()]);
  }, [refreshRules, refreshSelectedWorkspace, refreshWorkspace]);

  useWorkspaceQueryInvalidation({
    workspaceId,
    collection: [],
    current: [workspace, rules],
  });

  return {
    workspaceId,
    listeners: workspace.data?.listeners ?? [],
    rules: rules.data ?? [],
    error: selectedWorkspace.error ?? workspace.error ?? rules.error,
    isLoading: selectedWorkspace.isLoading || workspace.isLoading || rules.isLoading,
    refresh,
  };
}
