"use client";

import { useEffect, useRef, useState } from "react";
import { toast } from "@heroui/react";
import type {
  CertificateItemViewModel,
  ListenerCertificateDetailViewModel,
  ListenerDraft,
  ListenerDraftValidationViewModel,
  ListenerOverviewViewModel,
  ListenerProtocolPackageCatalogViewModel,
  ListenerUpstreamConnectionTestViewModel,
  ProxyWorkspace,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useAppEventRefresh, useBootstrap } from "@/features/shell/bootstrap-context";
import {
  useWorkspaceNavigation,
  useWorkspaceQueryInvalidation,
} from "@/features/shell/workspace-navigation";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useSelectedWorkspace } from "@/features/shell/selected-workspace-context";
import { toneColor } from "@/lib/format";
import type { ListenerOperationError } from "./listener-feedback";
import { socketUpstreamTls } from "./listener-data-plane";
import type { ListenerPending } from "./listener-runtime-card";
import {
  listenerCertificateReferences,
  type ListenerWorkspaceDraft,
  mergeCertificateDetails,
  mergePersistedListenerDeletion,
  reconcileListenerDraft,
  sameWorkspace,
  toListenerWorkspaceDraft,
} from "./listener-workspace-draft";
import { useListenerCertificates } from "./use-listener-certificates";
import { useListenerPersistence } from "./use-listener-persistence";
import { useListenerDraftTransition } from "./use-listener-draft-transition";
import { isListenerProtocolPackageCatalog, updateServerTls } from "./socket-listener-model";
import { hasUpstream, isFieldErrorRecord, isLocalResponder } from "./listener-view-model";

export function useListenerController() {
  const { navigate } = useWorkspaceNavigation();
  const { bootstrap } = useBootstrap();
  const selectedWorkspace = useSelectedWorkspace();
  const currentId = selectedWorkspace.selected?.id;
  const workspaceQuery = useIpcQuery<ProxyWorkspace>(
    `listener-workspace:${currentId ?? "none"}`,
    () => callCommand(commands.workspaceGet(currentId!)),
    undefined,
    { enabled: Boolean(currentId) },
  );
  const listenerOverview = useIpcQuery<ListenerOverviewViewModel>(
    `listener-overview:${currentId ?? "none"}`,
    () => callCommand(commands.listenerOverview(currentId!)),
    undefined,
    { enabled: Boolean(currentId) },
  );
  const certificateDetails = useIpcQuery<ListenerCertificateDetailViewModel[]>(
    `listener-certificate-overview:${currentId ?? "none"}`,
    () => callCommand(commands.listenerCertificateOverview(currentId!)),
    [],
    { enabled: Boolean(currentId) },
  );
  const protocolCatalog = useIpcQuery<ListenerProtocolPackageCatalogViewModel>(
    "listener-protocol-package-catalog",
    async () => {
      const catalog: unknown = await callCommand(commands.listenerProtocolPackageCatalog());
      if (!isListenerProtocolPackageCatalog(catalog)) {
        // useIpcQuery 统一通过 errorMessage 呈现错误；保留最小结构化边界，
        // 使本地响应校验失败与 Rust 目录失败走同一个明确 Alert。
        throw Object.assign(
          new Error("入口协议包目录数据不完整，请刷新后重试。"),
          { field_errors: {} },
        );
      }
      return catalog;
    },
  );
  const [workspace, setWorkspace] = useState<ListenerWorkspaceDraft>();
  const workspaceRef = useRef<ListenerWorkspaceDraft | undefined>(undefined);
  const authoritativeWorkspace = useRef<ProxyWorkspace | undefined>(undefined);
  const [conflictingListenerIds, setConflictingListenerIds] = useState<string[]>([]);
  const conflictingListenerIdsRef = useRef<string[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const selectionScopeVersion = useRef(0);
  const [validation, setValidation] = useState<ListenerDraftValidationViewModel>();
  const [pending, setPending] = useState<ListenerPending>();
  const [tlsTest, setTlsTest] = useState<ListenerUpstreamConnectionTestViewModel>();
  const [tlsTestError, setTlsTestError] = useState<string>();
  const [basicUsername, setBasicUsername] = useState("");
  const [basicPassword, setBasicPassword] = useState("");
  const [operationError, setOperationError] = useState<ListenerOperationError>();

  useWorkspaceQueryInvalidation({
    workspaceId: currentId,
    collection: [],
    current: [workspaceQuery, listenerOverview, certificateDetails],
  });
  const effectiveWorkspace: ListenerWorkspaceDraft | undefined = workspace?.id === currentId
    ? workspace
    : workspaceQuery.data ? toListenerWorkspaceDraft(workspaceQuery.data) : undefined;
  const effectiveIndex = Math.min(selectedIndex, Math.max(0, (effectiveWorkspace?.listeners.length ?? 1) - 1));
  const selected = effectiveWorkspace?.listeners[effectiveIndex];
  const selectedHasDraftConflict = Boolean(
    selected && conflictingListenerIds.includes(selected.id),
  );
  const selectedStatus = listenerOverview.data?.rows.find((row) => row.listener_id === selected?.id);
  const selectedStatusKnown = Boolean(selectedStatus)
    && !listenerOverview.error
    && !listenerOverview.isLoading;
  const selectedIsPersisted = Boolean(
    selected && workspaceQuery.data?.listeners.some((listener) => listener.id === selected.id),
  );
  // 已保存 Listener 的运行态配置来自启动快照。状态未知也必须 fail-closed，
  // 防止把编辑结果误认为已应用到正在工作的连接。
  const snapshotLocked = selectedIsPersisted
    && (!selectedStatusKnown || selectedStatus?.state !== "stopped");
  // 命令进行中也冻结编辑器，避免 deferred save/start 返回后覆盖用户刚输入的新草稿。
  const editorLocked = snapshotLocked || Boolean(pending);
  const selectedCanDelete = Boolean(selected) && (
    !selectedHasDraftConflict && (
      !selectedIsPersisted
      || (!snapshotLocked && selectedStatus?.state === "stopped")
    )
  );
  const hasUnsavedChanges = workspace !== undefined
    && workspace.id === currentId
    && workspaceQuery.data !== undefined
    && !sameWorkspace(workspace, workspaceQuery.data);
  const certificateActions = useListenerCertificates({
    currentId,
    workspace: effectiveWorkspace,
    selected,
    selectedIndex: effectiveIndex,
    pending,
    persistedReferences: workspaceQuery.data?.certificate_references ?? [],
    setWorkspace,
    clearDerivedResults,
    runPending: withPending,
  });
  const draftTransitions = useListenerDraftTransition({
    getContext: () => {
      if (!effectiveWorkspace || !selected || pending || snapshotLocked) return undefined;
      return {
        workspaceId: effectiveWorkspace.id,
        workspaceRevision: effectiveWorkspace.revision,
        listener: selected,
        scopeVersion: selectionScopeVersion.current,
      };
    },
    apply: (draft) => {
      const current = workspaceRef.current ?? effectiveWorkspace;
      if (!current || current.id !== effectiveWorkspace?.id) return;
      const next = {
        ...current,
        listeners: current.listeners.map((listener) =>
          listener.id === draft.id ? draft : listener),
      };
      workspaceRef.current = certificateActions.applyDraftWorkspace(next, current);
      clearDerivedResults();
    },
    onError: reportOperationError,
  });
  const invalidateDraftTransitions = draftTransitions.invalidate;
  const persistenceActions = useListenerPersistence({
    workspace: effectiveWorkspace,
    selected,
    status: selectedStatus,
    statusKnown: selectedStatusKnown,
    snapshotLocked,
    pending,
    hasUnsavedChanges,
    hasDraftConflict: selectedHasDraftConflict,
    transitionPending: draftTransitions.isTransitioning,
    leases: certificateActions.leases,
    setWorkspace,
    setPersistedWorkspace: workspaceQuery.setData,
    setValidation,
    refreshOverview: listenerOverview.refresh,
    refreshWorkspaces: selectedWorkspace.refresh,
    runPending: withPending,
  });

  useAppEventRefresh(
    ["listener_status_changed"],
    async () => {
      draftTransitions.invalidate();
      await listenerOverview.refresh();
    },
  );
  useAppEventRefresh(
    ["workspace_changed", "protocol_package_catalog_changed", "snapshot_required"],
    async () => {
      draftTransitions.invalidate();
      await protocolCatalog.refresh();
    },
  );

  useEffect(() => {
    workspaceRef.current = workspace;
  }, [workspace]);

  useEffect(() => {
    conflictingListenerIdsRef.current = conflictingListenerIds;
  }, [conflictingListenerIds]);

  useEffect(() => {
    const latest = workspaceQuery.data;
    if (!latest) return;
    invalidateDraftTransitions();
    const base = authoritativeWorkspace.current;
    authoritativeWorkspace.current = latest;
    const draft = workspaceRef.current;
    if (!base || base.id !== latest.id || !draft || draft.id !== latest.id) {
      conflictingListenerIdsRef.current = [];
      setConflictingListenerIds([]);
      return;
    }
    const reconciled = reconcileListenerDraft(
      draft,
      base,
      latest,
      conflictingListenerIdsRef.current,
    );
    workspaceRef.current = reconciled.workspace;
    conflictingListenerIdsRef.current = reconciled.conflictingListenerIds;
    setWorkspace(reconciled.workspace);
    setConflictingListenerIds(reconciled.conflictingListenerIds);
  }, [workspaceQuery.data, invalidateDraftTransitions]);
  const effectiveCertificateDetails = mergeCertificateDetails(
    certificateDetails.data ?? [],
    certificateActions.importedDetails,
  );
  const installationRoot = bootstrap?.certificate.items.find(
    (item): item is CertificateItemViewModel => item.kind === "local_root_ca",
  );

  function clearDerivedResults() {
    setValidation(undefined);
    setTlsTest(undefined);
    setTlsTestError(undefined);
    setOperationError(undefined);
  }

  function changeBasicUsername(value: string) {
    setBasicUsername(value);
    clearDerivedResults();
  }

  function changeBasicPassword(value: string) {
    setBasicPassword(value);
    clearDerivedResults();
  }

  function replaceSelected(changes: Partial<ListenerDraft>) {
    if (!effectiveWorkspace || !selected || editorLocked) return;
    draftTransitions.invalidate();
    workspaceRef.current = certificateActions.applyDraftWorkspace({
      ...effectiveWorkspace,
      listeners: effectiveWorkspace.listeners.map((listener, index) =>
        index === effectiveIndex ? { ...listener, ...changes } : listener,
      ),
    }, effectiveWorkspace);
    clearDerivedResults();
  }

  function discardSelectedConflictingDraft() {
    if (!effectiveWorkspace || !selected || !selectedHasDraftConflict) return;
    const latest = authoritativeWorkspace.current;
    if (!latest || latest.id !== effectiveWorkspace.id) return;
    const authoritativeListener = latest.listeners.find((listener) => listener.id === selected.id);
    const listeners = authoritativeListener
      ? effectiveWorkspace.listeners.map((listener) =>
        listener.id === selected.id ? authoritativeListener : listener)
      : effectiveWorkspace.listeners.filter((listener) => listener.id !== selected.id);
    certificateActions.applyDraftWorkspace({
      ...effectiveWorkspace,
      revision: latest.revision,
      listeners,
    }, effectiveWorkspace);
    const remainingConflicts = conflictingListenerIds.filter((id) => id !== selected.id);
    conflictingListenerIdsRef.current = remainingConflicts;
    setConflictingListenerIds(remainingConflicts);
    if (!authoritativeListener) {
      setSelectedIndex(Math.min(effectiveIndex, Math.max(0, listeners.length - 1)));
    }
    clearDerivedResults();
  }

  async function addListener() {
    if (!effectiveWorkspace || pending) return;
    await withPending("save", async () => {
      const draft = await callCommand(commands.listenerNew());
      setWorkspace({ ...effectiveWorkspace, listeners: [...effectiveWorkspace.listeners, draft] });
      setSelectedIndex(effectiveWorkspace.listeners.length);
      clearDerivedResults();
    });
  }

  async function copySelectedListener() {
    if (!effectiveWorkspace || !selected || pending) return;
    const copiedLocalResponder = isLocalResponder(selected);
    await withPending("save", async () => {
      const draft = await callCommand(commands.listenerDraftCopy(selected));
      setWorkspace({ ...effectiveWorkspace, listeners: [...effectiveWorkspace.listeners, draft] });
      setSelectedIndex(effectiveWorkspace.listeners.length);
      clearDerivedResults();
      toast(copiedLocalResponder
        ? "已创建独立监听副本，请检查监听端口、协议包和响应规则。"
        : "已创建独立监听副本，请修改监听端口和转发目标。", { variant: "success" });
    });
  }

  async function removeSelectedListener() {
    if (!effectiveWorkspace || !selected || !selectedCanDelete || pending) return;
    const persisted = workspaceQuery.data?.listeners.some((listener) => listener.id === selected.id);
    if (!persisted) {
      certificateActions.applyDraftWorkspace({
        ...effectiveWorkspace,
        listeners: effectiveWorkspace.listeners.filter((_, index) => index !== effectiveIndex),
      }, effectiveWorkspace);
      setSelectedIndex(Math.max(0, effectiveIndex - 1));
      clearDerivedResults();
      return;
    }
    await withPending("delete", async () => {
      await callCommand(commands.listenerDelete(effectiveWorkspace.id, effectiveWorkspace.revision, selected.id));
      const refreshed = await callCommand(commands.workspaceGet(effectiveWorkspace.id));
      certificateActions.applyDraftWorkspace(
        mergePersistedListenerDeletion(effectiveWorkspace, refreshed, selected.id),
        effectiveWorkspace,
      );
      workspaceQuery.setData(refreshed);
      setSelectedIndex(Math.min(effectiveIndex, Math.max(0, refreshed.listeners.length - 1)));
      clearDerivedResults();
      toast("代理监听已删除。", { variant: "success" });
      await listenerOverview.refresh();
      await selectedWorkspace.refresh();
    });
  }

  async function storeBasicCredential() {
    if (!selected || selected.data_plane.kind !== "http" || pending) return;
    const settings = selected.data_plane.settings;
    await withPending("secret", async () => {
      const credential = await callCommand(commands.workspaceSecretStoreBasic(basicUsername, basicPassword));
      replaceSelected({ data_plane: {
        kind: "http",
        settings: {
          ...settings,
          authentication: { mode: "basic", credential },
        },
      } });
      setBasicPassword("");
      toast("认证凭据已由系统密钥保护。", { variant: "success" });
    });
  }

  async function testUpstreamTls() {
    if (!effectiveWorkspace || !selected || !hasUpstream(selected) || snapshotLocked || pending || draftTransitions.isTransitioning()) return;
    setTlsTest(undefined);
    setTlsTestError(undefined);
    await withPending("tls-test", async () => {
      const certificateReferences = listenerCertificateReferences(
        selected,
        effectiveWorkspace.certificate_references,
      );
      const result = await callCommand(commands.listenerDraftValidate(
        effectiveWorkspace.id,
        effectiveWorkspace.revision,
        selected,
        certificateReferences,
      ));
      setValidation(result);
      if (!result.valid || !result.normalized) return;
      // 连接测试只读取当前草稿并建立一次临时上游连接，不持久化 Workspace。
      // 因此其他 Listener 正在运行时也可以安全测试当前 Listener 的证书配置。
      const normalizedListener = result.normalized.listeners.find(
        (listener) => listener.id === selected.id,
      );
      if (!normalizedListener) throw new Error("当前代理监听已被删除，请刷新后重试。");
      const test = await callCommand(
        commands.listenerTestUpstreamConnection(
          result.normalized.id,
          result.normalized.revision,
          normalizedListener,
          listenerCertificateReferences(
            normalizedListener,
            result.normalized.certificate_references,
          ),
        ),
      );
      const tlsServerName = test.tls_server_name_candidates?.[0];
      if (tlsServerName && selected.data_plane.kind === "socket") {
        const settings = selected.data_plane.settings;
        if (settings.topology.mode === "relay") {
          const upstreamTls = socketUpstreamTls(settings.topology.settings.security);
          if (upstreamTls) {
            const updated = {
              ...selected,
              data_plane: {
                kind: "socket" as const,
                settings: updateServerTls(settings, {
                  ...upstreamTls,
                  tls_server_name: tlsServerName,
                }),
              },
            };
            certificateActions.applyDraftWorkspace({
              ...effectiveWorkspace,
              listeners: effectiveWorkspace.listeners.map((listener) =>
                listener.id === selected.id ? updated : listener),
            }, effectiveWorkspace);
          }
        }
      }
      setTlsTest(test);
      toast(test.message, { variant: toneColor(test.ui_tone) });
    }, (reason) => setTlsTestError(errorMessage(reason)));
  }

  async function withPending(kind: ListenerPending, action: () => Promise<void>, onError?: (reason: unknown) => void) {
    draftTransitions.invalidate();
    setOperationError(undefined);
    setPending(kind);
    try { await action(); }
    catch (reason) {
      onError?.(reason);
      reportOperationError(reason);
    }
    finally { setPending(undefined); }
  }

  function reportOperationError(reason: unknown) {
    const appError = appErrorViewModel(reason);
    const fieldErrors = appError && isFieldErrorRecord(appError.field_errors)
      ? appError.field_errors
      : undefined;
    if (appError && fieldErrors && Object.keys(fieldErrors).length > 0) {
      setOperationError({ message: appError.message, fieldErrors });
    }
    const socketFieldError = selected?.data_plane.kind === "socket"
      && fieldErrors
      && Object.keys(fieldErrors).length > 0;
    toast(socketFieldError
      ? "操作未完成，请按页面提示修正 Socket 配置。"
      : appError && !fieldErrors
        ? "应用核心返回的错误结构不完整，请刷新后重试。"
        : errorMessage(reason), { variant: "danger" });
  }

  const editorFieldErrors = selected?.data_plane.kind === "socket"
    ? operationError?.fieldErrors ?? validation?.field_errors
    : validation?.field_errors;

  function selectListener(index: number) {
    selectionScopeVersion.current += 1;
    draftTransitions.invalidate();
    setSelectedIndex(index);
    clearDerivedResults();
  }

  return {
    navigate, effectiveWorkspace, effectiveIndex, selected, selectedHasDraftConflict,
    selectedStatus, selectedCanDelete, snapshotLocked, editorLocked, validation,
    pending, tlsTest, tlsTestError, basicUsername, basicPassword, operationError,
    editorFieldErrors, effectiveCertificateDetails, installationRoot,
    workspaceQuery, listenerOverview, certificateDetails, protocolCatalog,
    draftTransitions, persistenceActions, certificateActions,
    selectListener, addListener, copySelectedListener, removeSelectedListener,
    discardSelectedConflictingDraft, changeBasicUsername, changeBasicPassword,
    replaceSelected, storeBasicCredential, testUpstreamTls,
  };
}
