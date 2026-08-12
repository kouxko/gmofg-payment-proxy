"use client";

import { useState } from "react";
import { Alert, Button, Chip, toast } from "@heroui/react";
import type {
  CertificateItemViewModel,
  ListenerCertificateDetailViewModel,
  ListenerOverviewViewModel,
  ListenerUpstreamConnectionTestViewModel,
  ProxyListener,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
  WorkspaceValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useAppEventRefresh, useBootstrap } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ListenerEditor } from "./listener-editor";
import { ListenerListPanel } from "./listener-list-panel";
import { ListenerRuntimeCard, type ListenerPending } from "./listener-runtime-card";
import {
  listenerCertificateReferences,
  mergeCertificateDetails,
  mergePersistedListenerDeletion,
  sameWorkspace,
} from "./listener-workspace-draft";
import { useListenerCertificates } from "./use-listener-certificates";
import { useListenerPersistence } from "./use-listener-persistence";

export function ListenersView() {
  const { navigate } = useWorkspaceNavigation();
  const { bootstrap } = useBootstrap();
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>("listener-workspaces", () => callCommand(commands.workspaceList()));
  const currentId = workspaces.data?.find((item) => item.selected)?.id ?? workspaces.data?.[0]?.id;
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
  const [workspace, setWorkspace] = useState<ProxyWorkspace>();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [validation, setValidation] = useState<WorkspaceValidationViewModel>();
  const [pending, setPending] = useState<ListenerPending>();
  const [tlsTest, setTlsTest] = useState<ListenerUpstreamConnectionTestViewModel>();
  const [tlsTestError, setTlsTestError] = useState<string>();
  const [basicUsername, setBasicUsername] = useState("");
  const [basicPassword, setBasicPassword] = useState("");

  useAppEventRefresh(
    ["workspace_changed", "listener_status_changed", "snapshot_required"],
    listenerOverview.refresh,
  );

  const effectiveWorkspace = workspace?.id === currentId ? workspace : workspaceQuery.data;
  const effectiveIndex = Math.min(selectedIndex, Math.max(0, (effectiveWorkspace?.listeners.length ?? 1) - 1));
  const selected = effectiveWorkspace?.listeners[effectiveIndex];
  const selectedStatus = listenerOverview.data?.rows.find((row) => row.listener_id === selected?.id);
  const selectedStatusKnown = Boolean(selectedStatus) && !listenerOverview.error;
  const selectedIsPersisted = Boolean(
    selected && workspaceQuery.data?.listeners.some((listener) => listener.id === selected.id),
  );
  const selectedCanDelete = Boolean(selected) && (
    !selectedIsPersisted
    || (selectedStatusKnown
      && selectedStatus?.can_start === true
      && selectedStatus?.can_stop === false)
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
  const persistenceActions = useListenerPersistence({
    workspace: effectiveWorkspace,
    selected,
    status: selectedStatus,
    statusKnown: selectedStatusKnown,
    pending,
    hasUnsavedChanges,
    leases: certificateActions.leases,
    setWorkspace,
    setPersistedWorkspace: workspaceQuery.setData,
    setValidation,
    refreshOverview: listenerOverview.refresh,
    refreshWorkspaces: workspaces.refresh,
    runPending: withPending,
  });
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
  }

  function replaceSelected(changes: Partial<ProxyListener>) {
    if (!effectiveWorkspace || !selected) return;
    certificateActions.applyDraftWorkspace({
      ...effectiveWorkspace,
      listeners: effectiveWorkspace.listeners.map((listener, index) =>
        index === effectiveIndex ? { ...listener, ...changes } : listener,
      ),
    }, effectiveWorkspace);
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
    await withPending("save", async () => {
      const draft = await callCommand(commands.listenerCopy(selected));
      setWorkspace({ ...effectiveWorkspace, listeners: [...effectiveWorkspace.listeners, draft] });
      setSelectedIndex(effectiveWorkspace.listeners.length);
      clearDerivedResults();
      toast("已创建独立监听副本，请修改监听端口和转发目标。", { variant: "success" });
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
      await workspaces.refresh();
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
    if (!effectiveWorkspace || !selected || !hasUpstream(selected) || pending) return;
    setTlsTest(undefined);
    setTlsTestError(undefined);
    await withPending("tls-test", async () => {
      const certificateReferences = listenerCertificateReferences(
        selected,
        effectiveWorkspace.certificate_references,
      );
      const result = await callCommand(commands.listenerValidate(
        effectiveWorkspace.id,
        effectiveWorkspace.revision,
        selected,
        certificateReferences,
      ));
      setValidation(result);
      if (!result.valid) return;
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
      setTlsTest(test);
      toast(test.message, { variant: "success" });
    }, (reason) => setTlsTestError(errorMessage(reason)));
  }

  async function withPending(kind: ListenerPending, action: () => Promise<void>, onError?: (reason: unknown) => void) {
    setPending(kind);
    try { await action(); }
    catch (reason) { onError?.(reason); toast(errorMessage(reason), { variant: "danger" }); }
    finally { setPending(undefined); }
  }

  const errors = Object.entries(validation?.field_errors ?? {});

  return (
    <section className="grid h-full grid-cols-[420px_minmax(0,1fr)] max-[900px]:grid-cols-1">
      <ListenerListPanel
        listeners={effectiveWorkspace?.listeners ?? []}
        selectedIndex={effectiveIndex}
        loading={workspaceQuery.isLoading}
        error={workspaceQuery.error}
        disabled={!effectiveWorkspace || Boolean(pending)}
        onAdd={() => void addListener()}
        onSelect={(index) => { setSelectedIndex(index); setTlsTest(undefined); setTlsTestError(undefined); }}
        onNavigate={navigate}
      />
      <main className="min-w-0 space-y-5 overflow-auto p-5">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-xl font-semibold">监听配置</h2>
          {selected && <Chip color="accent" variant="soft">{listenerKind(selected)}</Chip>}
          <div className="ml-auto flex flex-wrap gap-2"><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void copySelectedListener()}>复制监听</Button><Button variant="danger-soft" isDisabled={!selected || !selectedCanDelete || Boolean(pending)} onPress={() => void removeSelectedListener()}>{pending === "delete" ? "删除中…" : "删除监听"}</Button><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void persistenceActions.validate()}>{pending === "validate" ? "校验中…" : "校验当前监听"}</Button><Button variant="primary" isDisabled={!selected || Boolean(pending)} onPress={() => void persistenceActions.save()}>{pending === "save" ? "保存中…" : "保存当前监听"}</Button></div>
        </div>
        {validation && (validation.valid ? <Alert status="success"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验通过</Alert.Title><Alert.Description>当前监听可保存、启动或执行上游 TLS 测试。</Alert.Description></Alert.Content></Alert> : <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验未通过</Alert.Title><Alert.Description>{errors.map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description></Alert.Content></Alert>)}
        {certificateDetails.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>证书详情读取失败</Alert.Title><Alert.Description>{certificateDetails.error}</Alert.Description></Alert.Content></Alert>}
        {!selected ? <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">选择一个代理监听进行编辑。</p> : <><ListenerRuntimeCard status={selectedStatus} isLoading={listenerOverview.isLoading} error={listenerOverview.error} pending={pending} onToggle={persistenceActions.toggleRuntime} onRetry={listenerOverview.refresh} /><ListenerEditor listener={selected} certificateReferences={effectiveWorkspace.certificate_references} certificateDetails={effectiveCertificateDetails} installationRoot={installationRoot} pending={pending} tlsTest={tlsTest} tlsTestError={tlsTestError} basicUsername={basicUsername} basicPassword={basicPassword} onBasicUsernameChange={setBasicUsername} onBasicPasswordChange={setBasicPassword} onChange={replaceSelected} onStoreBasicCredential={storeBasicCredential} onImportDownstreamServerIdentity={certificateActions.importDownstreamIdentity} onImportDownstreamClientTrust={certificateActions.importDownstreamTrust} onImportClientIdentity={certificateActions.importUpstreamIdentity} onImportServerTrust={certificateActions.importUpstreamTrust} onTestUpstreamTls={testUpstreamTls} /></>}
      </main>
    </section>
  );
}

function hasUpstream(listener: ProxyListener) {
  return listener.data_plane.kind === "socket"
    || listener.data_plane.settings.fixed_server !== null;
}

function listenerKind(listener: ProxyListener) {
  if (listener.data_plane.kind === "socket") {
    return `Socket · ${listener.data_plane.settings.security.mode}`;
  }
  return listener.data_plane.settings.fixed_server ? "HTTP · 固定 Server" : "HTTP · 按请求目标";
}
