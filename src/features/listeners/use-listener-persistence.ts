import { toast } from "@heroui/react";
import type {
  ListenerMonitorRowViewModel,
  ProxyListener,
  ProxyWorkspace,
  WorkspaceValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import type { ListenerPending } from "./listener-runtime-card";
import { listenerCertificateReferences, mergePersistedListener } from "./listener-workspace-draft";
import type { useDraftCertificateLeases } from "./use-draft-certificate-leases";

type RunPending = (
  kind: ListenerPending,
  action: () => Promise<void>,
  onError?: (reason: unknown) => void,
) => Promise<void>;

export function useListenerPersistence({
  workspace,
  selected,
  status,
  statusKnown,
  pending,
  hasUnsavedChanges,
  leases,
  setWorkspace,
  setPersistedWorkspace,
  setValidation,
  refreshOverview,
  refreshWorkspaces,
  runPending,
}: {
  workspace?: ProxyWorkspace;
  selected?: ProxyListener;
  status?: ListenerMonitorRowViewModel;
  statusKnown: boolean;
  pending?: ListenerPending;
  hasUnsavedChanges: boolean;
  leases: ReturnType<typeof useDraftCertificateLeases>;
  setWorkspace: (workspace: ProxyWorkspace) => void;
  setPersistedWorkspace: (workspace: ProxyWorkspace) => void;
  setValidation: (validation: WorkspaceValidationViewModel) => void;
  refreshOverview: () => Promise<void>;
  refreshWorkspaces: () => Promise<void>;
  runPending: RunPending;
}) {
  async function validate() {
    if (!workspace || !selected || pending) return;
    await runPending("validate", async () => {
      const result = await validateListener(workspace, selected);
      setValidation(result);
      if (result.valid) toast("当前监听校验通过。", { variant: "success" });
    });
  }

  async function save() {
    if (!workspace || !selected || pending) return;
    await runPending("save", async () => {
      const result = await validateListener(workspace, selected);
      setValidation(result);
      if (!result.valid) return;
      await persist(result.normalized, workspace, selected.id);
      toast("当前代理监听已保存。", { variant: "success" });
      await refreshWorkspaces();
    });
  }

  async function persist(normalized: ProxyWorkspace, localDraft: ProxyWorkspace, listenerId: string) {
    const listener = normalized.listeners.find((item) => item.id === listenerId);
    if (!listener) throw new Error("当前代理监听已被删除，请刷新后重试。");
    const references = listenerCertificateReferences(listener, normalized.certificate_references);
    const finishCertificateCommit = leases.beginCommit(references);
    let saved: ProxyWorkspace;
    try {
      saved = await callCommand(commands.listenerSave(
        normalized.id,
        normalized.revision,
        listener,
        references,
      ));
      finishCertificateCommit(saved.certificate_references);
    } catch (reason) {
      finishCertificateCommit();
      throw reason;
    }
    const merged = mergePersistedListener(localDraft, saved, listenerId);
    setWorkspace(merged);
    setPersistedWorkspace(saved);
    return merged;
  }

  async function toggleRuntime() {
    if (!workspace || !selected || !statusKnown || pending) return;
    const operation = status?.can_stop ? "stop" : status?.can_start ? "start" : undefined;
    if (!operation) return;
    await runPending(operation, async () => {
      let revision = workspace.revision;
      let draftSnapshot = workspace;
      if (operation === "start" && hasUnsavedChanges) {
        const result = await validateListener(workspace, selected);
        setValidation(result);
        if (!result.valid) return;
        draftSnapshot = await persist(result.normalized, workspace, selected.id);
        revision = draftSnapshot.revision;
      }
      const nextStatus = operation === "stop"
        ? await callCommand(commands.listenerStop(workspace.id, revision, selected.id))
        : await callCommand(commands.listenerStart(workspace.id, revision, selected.id));
      toast(`代理监听${nextStatus.state_text}。`, {
        variant: nextStatus.state === "faulted" ? "danger" : "success",
      });
      const refreshed = await callCommand(commands.workspaceGet(workspace.id));
      setWorkspace(mergePersistedListener(draftSnapshot, refreshed, selected.id));
      setPersistedWorkspace(refreshed);
      await refreshOverview();
      await refreshWorkspaces();
    });
  }

  return { validate, save, toggleRuntime };
}

function validateListener(workspace: ProxyWorkspace, listener: ProxyListener) {
  return callCommand(commands.listenerValidate(
    workspace.id,
    workspace.revision,
    listener,
    listenerCertificateReferences(listener, workspace.certificate_references),
  ));
}
