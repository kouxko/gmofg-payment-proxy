"use client";

import { useCallback, useMemo } from "react";
import type {
  ProtocolDocumentRuleDefinition,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import {
  isProtocolRuleList,
  protocolRuleListeners,
  type ProtocolRuleKind,
} from "./protocol-rule-model";

export function useProtocolRuleSource(kind: ProtocolRuleKind) {
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "protocol-rule-workspaces",
    () => callCommand(commands.workspaceList()),
  );
  const workspaceId = Array.isArray(workspaces.data)
    ? workspaces.data.find((workspace) => workspace.selected)?.id
    : undefined;
  const workspace = useIpcQuery<ProxyWorkspace>(
    `protocol-rule-workspace:${workspaceId ?? "none"}`,
    () => callCommand(commands.workspaceGet(workspaceId!)),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const ruleQuery = useIpcQuery<ProtocolDocumentRuleDefinition[]>(
    `protocol-rule-list:${workspaceId ?? "none"}`,
    () => callCommand(commands.protocolRuleList()),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const listeners = useMemo(
    () => protocolRuleListeners(
      Array.isArray(workspace.data?.listeners) ? workspace.data.listeners : [],
      kind,
    ),
    [kind, workspace.data],
  );
  const listenerIds = useMemo(
    () => new Set(listeners.map((listener) => listener.id)),
    [listeners],
  );
  const validRules = ruleQuery.data === undefined || isProtocolRuleList(ruleQuery.data);
  const rules = useMemo(
    () => validRules && Array.isArray(ruleQuery.data)
      ? ruleQuery.data.filter((rule) => listenerIds.has(rule.listener_id))
      : [],
    [listenerIds, ruleQuery.data, validRules],
  );
  const refreshWorkspaces = workspaces.refresh;
  const refreshWorkspace = workspace.refresh;
  const refreshRules = ruleQuery.refresh;
  const refresh = useCallback(
    async () => {
      await Promise.all([refreshWorkspaces(), refreshWorkspace(), refreshRules()]);
    },
    [refreshRules, refreshWorkspace, refreshWorkspaces],
  );
  useAppEventRefresh(["workspace_changed", "snapshot_required"], refresh);

  return {
    workspaceId,
    listeners,
    rules,
    error: workspaces.error
      ?? workspace.error
      ?? ruleQuery.error
      ?? (!validRules ? "报文规则列表包含无效数据，已拒绝显示。" : undefined),
    isLoading: workspaces.isLoading || workspace.isLoading || ruleQuery.isLoading,
    refresh,
  };
}

export type ProtocolRuleSource = ReturnType<typeof useProtocolRuleSource>;
