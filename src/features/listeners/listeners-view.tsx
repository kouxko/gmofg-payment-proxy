"use client";

import { useState } from "react";
import { Alert, Button, Chip, toast } from "@heroui/react";
import type {
  CertificateItemViewModel,
  ListenerCertificateDetailViewModel,
  ListenerOverviewViewModel,
  ListenerProtocolPackageCatalogViewModel,
  ListenerUpstreamConnectionTestViewModel,
  ProxyListener,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
  WorkspaceValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useAppEventRefresh, useBootstrap } from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { appErrorViewModel, callCommand, errorMessage } from "@/lib/ipc/client";
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
import { isListenerProtocolPackageCatalog, socketWorkingMode } from "./socket-listener-model";

export function ListenersView() {
  const { navigate } = useWorkspaceNavigation();
  const { bootstrap } = useBootstrap();
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>("listener-workspaces", () => callCommand(commands.workspaceList()));
  // Listener 页面只跟随后端明确选中的 Workspace；缺失选择时不读取其他 Workspace。
  const currentId = workspaces.data?.find((item) => item.selected)?.id;
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
  const [workspace, setWorkspace] = useState<ProxyWorkspace>();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [validation, setValidation] = useState<WorkspaceValidationViewModel>();
  const [pending, setPending] = useState<ListenerPending>();
  const [tlsTest, setTlsTest] = useState<ListenerUpstreamConnectionTestViewModel>();
  const [tlsTestError, setTlsTestError] = useState<string>();
  const [basicUsername, setBasicUsername] = useState("");
  const [basicPassword, setBasicPassword] = useState("");
  const [operationError, setOperationError] = useState<{
    message: string;
    fieldErrors: Record<string, string[]>;
  }>();

  useAppEventRefresh(
    ["workspace_changed", "listener_status_changed", "snapshot_required"],
    listenerOverview.refresh,
  );
  useAppEventRefresh(
    ["workspace_changed", "snapshot_required"],
    protocolCatalog.refresh,
  );

  const effectiveWorkspace = workspace?.id === currentId ? workspace : workspaceQuery.data;
  const effectiveIndex = Math.min(selectedIndex, Math.max(0, (effectiveWorkspace?.listeners.length ?? 1) - 1));
  const selected = effectiveWorkspace?.listeners[effectiveIndex];
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
    !selectedIsPersisted
    || (!snapshotLocked && selectedStatus?.state === "stopped")
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
    snapshotLocked,
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

  function replaceSelected(changes: Partial<ProxyListener>) {
    if (!effectiveWorkspace || !selected || editorLocked) return;
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
    const copiedLocalResponder = isLocalResponder(selected);
    await withPending("save", async () => {
      const draft = await callCommand(commands.listenerCopy(selected));
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
    if (!effectiveWorkspace || !selected || !hasUpstream(selected) || snapshotLocked || pending) return;
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
    setOperationError(undefined);
    setPending(kind);
    try { await action(); }
    catch (reason) {
      onError?.(reason);
      const appError = appErrorViewModel(reason);
      const fieldErrors = appError && isFieldErrorRecord(appError.field_errors)
        ? appError.field_errors
        : undefined;
      if (appError && fieldErrors && Object.keys(fieldErrors).length > 0) {
        // 保存/启动错误不是 WorkspaceValidation，独立保存 Rust 原始字段路径与消息。
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
    finally { setPending(undefined); }
  }

  const errors = Object.entries(validation?.field_errors ?? {});
  const editorFieldErrors = selected?.data_plane.kind === "socket"
    ? operationError?.fieldErrors ?? validation?.field_errors
    : validation?.field_errors;

  return (
    <section className="grid h-full grid-cols-[420px_minmax(0,1fr)] max-[900px]:grid-cols-1">
      <ListenerListPanel
        listeners={effectiveWorkspace?.listeners ?? []}
        selectedIndex={effectiveIndex}
        loading={workspaceQuery.isLoading}
        error={workspaceQuery.error}
        disabled={!effectiveWorkspace || Boolean(pending)}
        onAdd={() => void addListener()}
        onSelect={(index) => { setSelectedIndex(index); clearDerivedResults(); }}
        onNavigate={navigate}
      />
      <main className="min-w-0 space-y-5 overflow-auto p-5">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-xl font-semibold">监听配置</h2>
          {selected && <Chip color="accent" variant="soft">{listenerKind(selected)}</Chip>}
          <div className="ml-auto flex flex-wrap gap-2"><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void copySelectedListener()}>复制监听</Button><Button variant="danger-soft" isDisabled={!selected || !selectedCanDelete || snapshotLocked || Boolean(pending)} onPress={() => void removeSelectedListener()}>{pending === "delete" ? "删除中…" : "删除监听"}</Button><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void persistenceActions.validate()}>{pending === "validate" ? "校验中…" : "校验当前监听"}</Button><Button variant="primary" isDisabled={!selected || snapshotLocked || Boolean(pending)} onPress={() => void persistenceActions.save()}>{pending === "save" ? "保存中…" : "保存当前监听"}</Button></div>
        </div>
        {validation && (validation.valid ? <Alert status="success"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验通过</Alert.Title><Alert.Description>{isLocalResponder(selected) ? "配置结构有效，可保存并启动；收到请求后将由本机生成响应。" : "当前监听可保存、启动或执行上游 TLS 测试。"}</Alert.Description></Alert.Content></Alert> : <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验未通过</Alert.Title><Alert.Description>{selected?.data_plane.kind === "socket" ? "请按下方分类修正 Socket 配置。" : errors.map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description></Alert.Content></Alert>)}
        {operationError && <Alert status="danger"><Alert.Indicator /><Alert.Content>
          <Alert.Title>{selected?.data_plane.kind === "socket" ? "操作未完成" : operationError.message}</Alert.Title>
          <Alert.Description>{selected?.data_plane.kind === "socket" ? <>
            <p>请按下方分类修正 Socket 配置。</p>
            <details className="mt-2"><summary className="cursor-pointer font-medium">高级诊断</summary>
              <p className="mt-1">{operationError.message}</p>
              <ul>{Object.entries(operationError.fieldErrors).map(([field, messages]) => <li key={field}>{field}: {messages.join("，")}</li>)}</ul>
            </details>
          </> : Object.entries(operationError.fieldErrors).map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description>
        </Alert.Content></Alert>}
        {certificateDetails.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>证书详情读取失败</Alert.Title><Alert.Description>{certificateDetails.error}</Alert.Description></Alert.Content></Alert>}
        {!selected ? <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">选择一个代理监听进行编辑。</p> : <><ListenerRuntimeCard status={selectedStatus} isLoading={listenerOverview.isLoading} error={listenerOverview.error} pending={pending} onToggle={persistenceActions.toggleRuntime} onRetry={listenerOverview.refresh} /><ListenerEditor listener={selected} protocolCatalog={{ data: protocolCatalog.data, error: protocolCatalog.error, loading: protocolCatalog.isLoading, refresh: protocolCatalog.refresh }} locked={editorLocked} fieldErrors={editorFieldErrors} certificateReferences={effectiveWorkspace.certificate_references} certificateDetails={effectiveCertificateDetails} installationRoot={installationRoot} pending={pending} tlsTest={tlsTest} tlsTestError={tlsTestError} basicUsername={basicUsername} basicPassword={basicPassword} onBasicUsernameChange={changeBasicUsername} onBasicPasswordChange={changeBasicPassword} onChange={replaceSelected} onStoreBasicCredential={storeBasicCredential} onImportDownstreamServerIdentity={certificateActions.importDownstreamIdentity} onImportDownstreamClientTrust={certificateActions.importDownstreamTrust} onImportClientIdentity={certificateActions.importUpstreamIdentity} onImportServerTrust={certificateActions.importUpstreamTrust} onTestUpstreamTls={testUpstreamTls} /></>}
      </main>
    </section>
  );
}

function isLocalResponder(listener?: ProxyListener): boolean {
  return listener?.data_plane.kind === "socket"
    && listener.data_plane.settings.topology.mode === "local_responder";
}

function hasUpstream(listener: ProxyListener) {
  if (listener.data_plane.kind === "socket") {
    return listener.data_plane.settings.topology.mode === "relay";
  }
  return listener.data_plane.settings.fixed_server !== null;
}

function listenerKind(listener: ProxyListener) {
  if (listener.data_plane.kind === "socket") {
    const mode = socketWorkingMode(listener.data_plane.settings);
    if (mode === "raw_relay") return "Socket · 透明转发";
    if (mode === "protocol_relay") return "Socket · 按协议转发";
    if (mode === "local_response") return "Socket · 本地应答";
    return "Socket · 配置不兼容";
  }
  return listener.data_plane.settings.fixed_server ? "HTTP · 固定 Server" : "HTTP · 按请求目标";
}

function isFieldErrorRecord(value: unknown): value is Record<string, string[]> {
  return typeof value === "object"
    && value !== null
    && !Array.isArray(value)
    && Object.entries(value).every(([field, messages]) => field.length > 0
      && Array.isArray(messages)
      && messages.every((message) => typeof message === "string"));
}
