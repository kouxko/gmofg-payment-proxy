"use client";

import { useRef, useState } from "react";
import { Alert, Button, Chip, Spinner, Table, toast } from "@heroui/react";
import type {
  CertificateItemViewModel,
  CertificateReference,
  ListenerCertificateDetailViewModel,
  ListenerCertificateImportViewModel,
  ListenerMonitorRowViewModel,
  ListenerOverviewViewModel,
  ListenerUpstreamTlsTestViewModel,
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

type Pending = "validate" | "save" | "delete" | "start" | "stop" | "secret" | "tls-test"
  | "import-downstream-identity" | "import-downstream-trust"
  | "import-upstream-identity" | "import-upstream-trust";

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
  const [importedCertificateDetails, setImportedCertificateDetails] = useState<ListenerCertificateDetailViewModel[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [validation, setValidation] = useState<WorkspaceValidationViewModel>();
  const [pending, setPending] = useState<Pending>();
  const [tlsTest, setTlsTest] = useState<ListenerUpstreamTlsTestViewModel>();
  const [tlsTestError, setTlsTestError] = useState<string>();
  const [basicUsername, setBasicUsername] = useState("");
  const [basicPassword, setBasicPassword] = useState("");
  const certificateDiscardsInFlight = useRef(new Set<string>());

  useAppEventRefresh(
    ["workspace_changed", "listener_status_changed", "snapshot_required"],
    listenerOverview.refresh,
  );

  const effectiveWorkspace = workspace?.id === currentId ? workspace : workspaceQuery.data;
  const effectiveIndex = Math.min(selectedIndex, Math.max(0, (effectiveWorkspace?.listeners.length ?? 1) - 1));
  const selected = effectiveWorkspace?.listeners[effectiveIndex];
  const selectedStatus = listenerOverview.data?.rows.find((row) => row.listener_id === selected?.id);
  const selectedStatusKnown = Boolean(selectedStatus) && !listenerOverview.error;
  const selectedCanDelete = selectedStatusKnown
    && selectedStatus?.can_start === true
    && selectedStatus?.can_stop === false;
  const hasUnsavedChanges = workspace !== undefined
    && workspace.id === currentId
    && workspaceQuery.data !== undefined
    && !sameWorkspace(workspace, workspaceQuery.data);
  const effectiveCertificateDetails = mergeCertificateDetails(
    certificateDetails.data ?? [],
    importedCertificateDetails,
  );
  const installationLeaf = bootstrap?.certificate.items.find(
    (item): item is CertificateItemViewModel => item.kind === "proxy_leaf",
  );

  function clearDerivedResults() {
    setValidation(undefined);
    setTlsTest(undefined);
    setTlsTestError(undefined);
  }

  function replaceSelected(changes: Partial<ProxyListener>) {
    if (!effectiveWorkspace || !selected) return;
    applyDraftWorkspace({
      ...effectiveWorkspace,
      listeners: effectiveWorkspace.listeners.map((listener, index) =>
        index === effectiveIndex ? { ...listener, ...changes } : listener,
      ),
    }, effectiveWorkspace);
    clearDerivedResults();
  }

  function applyDraftWorkspace(next: ProxyWorkspace, previous: ProxyWorkspace) {
    const { workspace: pruned, detached } = pruneDetachedDraftCertificates(
      previous,
      next,
      workspaceQuery.data?.certificate_references ?? [],
    );
    setWorkspace(pruned);
    if (detached.length === 0) return;
    const detachedIds = new Set(detached.map((reference) => reference.id));
    setImportedCertificateDetails((current) =>
      current.filter((detail) => !detachedIds.has(detail.reference_id)),
    );
    for (const reference of detached) {
      discardDraftCertificate(reference);
    }
  }

  function discardDraftCertificate(reference: CertificateReference) {
    if (certificateDiscardsInFlight.current.has(reference.reference)) return;
    certificateDiscardsInFlight.current.add(reference.reference);
    void callCommand(commands.listenerCertificateDiscard(reference))
      .catch((reason) => toast(errorMessage(reason), { variant: "danger" }))
      .finally(() => certificateDiscardsInFlight.current.delete(reference.reference));
  }

  async function validateSelectedListener() {
    if (!effectiveWorkspace || !selected || pending) return;
    await withPending("validate", async () => {
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
      if (result.valid) toast("当前监听校验通过。", { variant: "success" });
    });
  }

  async function saveSelectedListener() {
    if (!effectiveWorkspace || !selected || pending) return;
    await withPending("save", async () => {
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
      await persistSelectedListener(result.normalized, effectiveWorkspace, selected.id);
      toast("当前代理监听已保存。", { variant: "success" });
      await workspaces.refresh();
    });
  }

  async function persistSelectedListener(
    normalized: ProxyWorkspace,
    localDraft: ProxyWorkspace,
    listenerId: string,
  ) {
    const listener = normalized.listeners.find((item) => item.id === listenerId);
    if (!listener) throw new Error("当前代理监听已被删除，请刷新后重试。");
    const saved = await callCommand(commands.listenerSave(
      normalized.id,
      normalized.revision,
      listener,
      normalized.certificate_references,
    ));
    const merged = mergePersistedListener(localDraft, saved, listenerId);
    setWorkspace(merged);
    workspaceQuery.setData(saved);
    return merged;
  }

  async function toggleListenerRuntime() {
    if (!effectiveWorkspace || !selected || !selectedStatusKnown || pending) return;
    const operation = selectedStatus?.can_stop
      ? "stop"
      : selectedStatus?.can_start
        ? "start"
        : undefined;
    if (!operation) return;
    await withPending(operation, async () => {
      let revision = effectiveWorkspace.revision;
      let draftSnapshot = effectiveWorkspace;
      if (operation === "start" && hasUnsavedChanges) {
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
        draftSnapshot = await persistSelectedListener(
          result.normalized,
          effectiveWorkspace,
          selected.id,
        );
        revision = draftSnapshot.revision;
      }
      const status = operation === "stop"
        ? await callCommand(commands.listenerStop(effectiveWorkspace.id, revision, selected.id))
        : await callCommand(commands.listenerStart(effectiveWorkspace.id, revision, selected.id));
      toast(`代理监听${status.state_text}。`, { variant: status.state === "faulted" ? "danger" : "success" });
      const refreshed = await callCommand(commands.workspaceGet(effectiveWorkspace.id));
      setWorkspace(mergePersistedListener(draftSnapshot, refreshed, selected.id));
      workspaceQuery.setData(refreshed);
      await listenerOverview.refresh();
      await workspaces.refresh();
    });
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
      applyDraftWorkspace({
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
      applyDraftWorkspace(
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
    if (!selected || pending) return;
    await withPending("secret", async () => {
      const credential = await callCommand(commands.workspaceSecretStoreBasic(basicUsername, basicPassword));
      replaceSelected({ authentication: { mode: "basic", credential } });
      setBasicPassword("");
      toast("认证凭据已由系统密钥保护。", { variant: "success" });
    });
  }

  async function testUpstreamTls() {
    if (!effectiveWorkspace || !selected?.fixed_server || pending) return;
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
      // TLS 测试只读取当前草稿并建立一次临时上游连接，不持久化 Workspace。
      // 因此其他 Listener 正在运行时也可以安全测试当前 Listener 的证书配置。
      const normalizedListener = result.normalized.listeners.find(
        (listener) => listener.id === selected.id,
      );
      if (!normalizedListener) throw new Error("当前代理监听已被删除，请刷新后重试。");
      const test = await callCommand(
        commands.listenerTestUpstreamTls(
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

  async function importDownstreamIdentity(label: string) {
    return importCertificate(
      "import-downstream-identity",
      () => callCommand(commands.listenerImportDownstreamServerIdentity(label)),
      (listener, referenceId) => ({
        ...listener,
        downstream_tls: { ...listener.downstream_tls, server_identity: referenceId },
      }),
    );
  }

  async function importDownstreamTrust(label: string) {
    return importCertificate(
      "import-downstream-trust",
      () => callCommand(commands.listenerImportDownstreamClientTrust(label)),
      (listener, referenceId) => {
        const mode = listener.downstream_tls.client_authentication.mode;
        return {
          ...listener,
          downstream_tls: {
            ...listener.downstream_tls,
            client_authentication: mode === "required"
              ? { mode: "required", trust: referenceId }
              : { mode: "optional", trust: referenceId },
          },
        };
      },
    );
  }

  async function importIdentity(label: string, password: string) {
    return importCertificate(
      "import-upstream-identity",
      () => callCommand(commands.listenerImportUpstreamClientIdentity(label, password)),
      (listener, referenceId) => listener.fixed_server ? {
        ...listener,
        fixed_server: {
          ...listener.fixed_server,
          upstream_tls: { ...listener.fixed_server.upstream_tls, client_identity: referenceId },
        },
      } : listener,
    );
  }

  async function importTrust(label: string) {
    return importCertificate(
      "import-upstream-trust",
      () => callCommand(commands.listenerImportUpstreamServerTrust(label)),
      (listener, referenceId) => listener.fixed_server ? {
        ...listener,
        fixed_server: {
          ...listener.fixed_server,
          upstream_tls: { ...listener.fixed_server.upstream_tls, server_trust: referenceId },
        },
      } : listener,
    );
  }

  async function importCertificate(
    kind: Extract<Pending, `import-${string}`>,
    load: () => Promise<ListenerCertificateImportViewModel | null>,
    bind: (listener: ProxyListener, referenceId: string) => ProxyListener,
  ) {
    if (!effectiveWorkspace || !selected || pending) return false;
    let importedSuccessfully = false;
    await withPending(kind, async () => {
      const result = await load();
      if (!result) return;
      const { reference, detail } = result;
      applyDraftWorkspace({
        ...effectiveWorkspace,
        listeners: effectiveWorkspace.listeners.map((listener, index) =>
          index === effectiveIndex ? bind(listener, reference.id) : listener,
        ),
        certificate_references: [
          ...effectiveWorkspace.certificate_references.filter((item) => item.id !== reference.id),
          reference,
        ],
      }, effectiveWorkspace);
      clearDerivedResults();
      setImportedCertificateDetails((current) => mergeCertificateDetails(current, [detail]));
      importedSuccessfully = true;
      toast("证书材料已安全导入并绑定到当前监听。", { variant: "success" });
    });
    return importedSuccessfully;
  }

  async function withPending(kind: Pending, action: () => Promise<void>, onError?: (reason: unknown) => void) {
    setPending(kind);
    try { await action(); }
    catch (reason) { onError?.(reason); toast(errorMessage(reason), { variant: "danger" }); }
    finally { setPending(undefined); }
  }

  const errors = Object.entries(validation?.field_errors ?? {});

  return (
    <section className="grid h-full grid-cols-[420px_minmax(0,1fr)] max-[900px]:grid-cols-1">
      <aside className="min-w-0 space-y-4 overflow-auto border-r border-[var(--telemetry-line)] p-5 max-[900px]:border-r-0 max-[900px]:border-b">
        <div><h1 className="text-2xl font-semibold">代理监听</h1><p className="mt-1 text-sm text-[var(--telemetry-muted)]">每个监听都可按请求目标转发，或选择转发到一个固定 Server。</p></div>
        <Button variant="primary" className="w-full" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void addListener()}>新建代理监听</Button>
        <Alert status="accent"><Alert.Indicator /><Alert.Content><Alert.Title>一个监听，两种请求去向</Alert.Title><Alert.Description>默认读取请求目标；启用“转发到固定 Server”后，可为当前监听单独配置 Server URL、CA、主机名校验和可选 mTLS 身份。</Alert.Description></Alert.Content></Alert>
        <Alert status="warning"><Alert.Indicator /><Alert.Content><Alert.Title>故障模拟与规则作用于监听流量</Alert.Title><Alert.Description>启动监听后，到故障模拟或拦截规则页面配置行为。</Alert.Description><div className="mt-3 flex gap-2"><Button size="sm" variant="primary" onPress={() => navigate("/faults")}>去添加故障模拟</Button><Button size="sm" variant="outline" onPress={() => navigate("/rules")}>去配置拦截规则</Button></div></Alert.Content></Alert>
        {workspaceQuery.isLoading && <Spinner aria-label="正在读取代理监听" />}
        {workspaceQuery.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>读取失败</Alert.Title><Alert.Description>{workspaceQuery.error}</Alert.Description></Alert.Content></Alert>}
        <ListenerTable listeners={effectiveWorkspace?.listeners ?? []} selectedIndex={effectiveIndex} onSelect={(index) => { setSelectedIndex(index); setTlsTest(undefined); setTlsTestError(undefined); }} />
      </aside>
      <main className="min-w-0 space-y-5 overflow-auto p-5">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-xl font-semibold">监听配置</h2>
          {selected && <Chip color="accent" variant="soft">{selected.fixed_server ? "固定 Server" : "按请求目标"}</Chip>}
          <div className="ml-auto flex flex-wrap gap-2"><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void copySelectedListener()}>复制监听</Button><Button variant="danger-soft" isDisabled={!selected || !selectedCanDelete || Boolean(pending)} onPress={() => void removeSelectedListener()}>{pending === "delete" ? "删除中…" : "删除监听"}</Button><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void validateSelectedListener()}>{pending === "validate" ? "校验中…" : "校验当前监听"}</Button><Button variant="primary" isDisabled={!selected || Boolean(pending)} onPress={() => void saveSelectedListener()}>{pending === "save" ? "保存中…" : "保存当前监听"}</Button></div>
        </div>
        {validation && (validation.valid ? <Alert status="success"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验通过</Alert.Title><Alert.Description>当前监听可保存、启动或执行上游 TLS 测试。</Alert.Description></Alert.Content></Alert> : <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>当前监听校验未通过</Alert.Title><Alert.Description>{errors.map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description></Alert.Content></Alert>)}
        {certificateDetails.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>证书详情读取失败</Alert.Title><Alert.Description>{certificateDetails.error}</Alert.Description></Alert.Content></Alert>}
        {!selected ? <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">选择一个代理监听进行编辑。</p> : <><ListenerRuntimeCard status={selectedStatus} isLoading={listenerOverview.isLoading} error={listenerOverview.error} pending={pending} onToggle={toggleListenerRuntime} onRetry={listenerOverview.refresh} /><ListenerEditor listener={selected} certificateReferences={effectiveWorkspace.certificate_references} certificateDetails={effectiveCertificateDetails} installationLeaf={installationLeaf} pending={pending} tlsTest={tlsTest} tlsTestError={tlsTestError} basicUsername={basicUsername} basicPassword={basicPassword} onBasicUsernameChange={setBasicUsername} onBasicPasswordChange={setBasicPassword} onChange={replaceSelected} onStoreBasicCredential={storeBasicCredential} onImportDownstreamServerIdentity={importDownstreamIdentity} onImportDownstreamClientTrust={importDownstreamTrust} onImportClientIdentity={importIdentity} onImportServerTrust={importTrust} onTestUpstreamTls={testUpstreamTls} /></>}
      </main>
    </section>
  );
}

function ListenerRuntimeCard({
  status,
  isLoading,
  error,
  pending,
  onToggle,
  onRetry,
}: {
  status?: ListenerMonitorRowViewModel;
  isLoading: boolean;
  error?: string;
  pending?: Pending;
  onToggle: () => Promise<void>;
  onRetry: () => Promise<void>;
}) {
  const unavailable = isLoading || Boolean(error) || !status;
  const operation = status?.can_stop ? "stop" : status?.can_start ? "start" : undefined;
  const stateText = isLoading
    ? "正在读取…"
    : error
      ? "查询失败"
      : status?.state_text ?? "未知（Rust 未返回当前监听状态）";
  const actionText = pending === "start"
    ? "启动中…"
    : pending === "stop"
      ? "停止中…"
      : unavailable
        ? "状态不可用"
        : operation === "stop"
          ? "停止监听"
          : operation === "start"
            ? "启动监听"
            : "无可用操作";

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--telemetry-line)] p-3">
      <div className="min-w-0">
        <p className="text-sm">运行状态：{stateText}</p>
        {error && <p className="mt-1 text-xs text-[var(--telemetry-danger)]">{error}</p>}
      </div>
      <div className="flex items-center gap-2">
        {error && <Button size="sm" variant="outline" onPress={() => void onRetry()}>重试状态查询</Button>}
        <Button
          variant={operation === "stop" ? "danger-soft" : "primary"}
          isDisabled={Boolean(pending) || unavailable || !operation}
          onPress={() => void onToggle()}
        >
          {actionText}
        </Button>
      </div>
    </div>
  );
}

function ListenerTable({ listeners, selectedIndex, onSelect }: { listeners: ProxyListener[]; selectedIndex: number; onSelect: (index: number) => void }) {
  return <Table><Table.ScrollContainer><Table.Content aria-label="代理监听列表"><Table.Header><Table.Column isRowHeader>监听名称</Table.Column><Table.Column>客户端连接 → 请求去向</Table.Column></Table.Header><Table.Body renderEmptyState={() => <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">当前工作区还没有代理监听</div>}>{listeners.map((listener, index) => <Table.Row key={listener.id} id={listener.id} onAction={() => onSelect(index)} className={index === selectedIndex ? "bg-[var(--telemetry-accent-soft)]" : ""}><Table.Cell><span className="font-medium">{listener.name}</span></Table.Cell><Table.Cell><div className="grid min-w-0 gap-1 font-mono text-xs"><span className="truncate">{listener.bind_address}:{listener.port}</span><span className="truncate text-[var(--telemetry-muted)]">→ {listener.fixed_server?.upstream_url || "请求中的目标地址"}</span></div></Table.Cell></Table.Row>)}</Table.Body></Table.Content></Table.ScrollContainer></Table>;
}

function sameWorkspace(left: ProxyWorkspace, right: ProxyWorkspace) {
  return JSON.stringify(left) === JSON.stringify(right);
}

/// Rust 只持久化当前监听；其他监听仍可能包含用户尚未保存的草稿。
/// 用新的 revision、证书引用和当前监听覆盖本地草稿，避免保存 B 时丢失 A 的未保存输入。
function mergePersistedListener(
  draft: ProxyWorkspace,
  persisted: ProxyWorkspace,
  listenerId: string,
) {
  const persistedListener = persisted.listeners.find((listener) => listener.id === listenerId);
  const draftIds = new Set(draft.listeners.map((listener) => listener.id));
  const listeners = draft.listeners
    .map((listener) => listener.id === listenerId && persistedListener ? persistedListener : listener);
  for (const listener of persisted.listeners) {
    if (!draftIds.has(listener.id)) listeners.push(listener);
  }
  const reachableIds = listenerCertificateReferenceIds(listeners);
  const persistedReferences = new Map(
    persisted.certificate_references.map((reference) => [reference.id, reference]),
  );
  for (const reference of draft.certificate_references) {
    if (reachableIds.has(reference.id) && !persistedReferences.has(reference.id)) {
      persistedReferences.set(reference.id, reference);
    }
  }
  return {
    ...draft,
    revision: persisted.revision,
    certificate_references: [...persistedReferences.values()],
    listeners,
  };
}

/**
 * Rust 删除命令只改变被删除的 Listener 及 Workspace revision。
 * 其他 Listener 的本地草稿必须继续保留，包括它们刚导入但尚未保存的托管证书引用。
 */
function mergePersistedListenerDeletion(
  draft: ProxyWorkspace,
  persisted: ProxyWorkspace,
  deletedListenerId: string,
) {
  const draftIds = new Set(draft.listeners.map((listener) => listener.id));
  const listeners = draft.listeners.filter((listener) => listener.id !== deletedListenerId);
  for (const listener of persisted.listeners) {
    if (!draftIds.has(listener.id)) listeners.push(listener);
  }
  const reachableIds = listenerCertificateReferenceIds(listeners);
  const references = new Map(
    persisted.certificate_references.map((reference) => [reference.id, reference]),
  );
  for (const reference of draft.certificate_references) {
    if (reachableIds.has(reference.id) && !references.has(reference.id)) {
      references.set(reference.id, reference);
    }
  }
  return {
    ...draft,
    revision: persisted.revision,
    listeners,
    certificate_references: [...references.values()],
  };
}

function mergeCertificateDetails(
  first: ListenerCertificateDetailViewModel[],
  second: ListenerCertificateDetailViewModel[],
) {
  const details = new Map(first.map((detail) => [detail.reference_id, detail]));
  for (const detail of second) details.set(detail.reference_id, detail);
  return [...details.values()];
}

/**
 * Listener 级命令只携带当前监听实际可达的安全引用。
 *
 * Workspace 中可能还有其他监听尚未保存的证书草稿；把整表交给 listener_save
 * 会把无关材料误判为当前监听的变更，也会扩大单监听命令的写入边界。
 */
function listenerCertificateReferences(
  listener: ProxyListener,
  references: ProxyWorkspace["certificate_references"],
) {
  const referencedIds = listenerCertificateReferenceIds([listener]);
  return references.filter((reference) => referencedIds.has(reference.id));
}

function listenerCertificateReferenceIds(listeners: ProxyListener[]) {
  const referencedIds = new Set<string>();
  for (const listener of listeners) {
    if (listener.downstream_tls.server_identity) {
      referencedIds.add(listener.downstream_tls.server_identity);
    }
    const clientAuthentication = listener.downstream_tls.client_authentication;
    if (clientAuthentication.mode !== "disabled" && clientAuthentication.trust) {
      referencedIds.add(clientAuthentication.trust);
    }
    const upstreamTls = listener.fixed_server?.upstream_tls;
    if (upstreamTls?.server_trust) referencedIds.add(upstreamTls.server_trust);
    if (upstreamTls?.client_identity) referencedIds.add(upstreamTls.client_identity);
  }
  return referencedIds;
}

/**
 * 导入命令会先把证书写入系统安全存储，再把非敏感引用交给页面草稿。
 * 当用户在保存前替换、清空或删除该草稿时，只清理不再被任何草稿监听使用、
 * 且尚未出现在持久化 Workspace 中的材料。
 */
function pruneDetachedDraftCertificates(
  previous: ProxyWorkspace,
  next: ProxyWorkspace,
  persistedReferences: CertificateReference[],
) {
  const reachableIds = listenerCertificateReferenceIds(next.listeners);
  const persistedIds = new Set(persistedReferences.map((reference) => reference.id));
  const persistedHandles = new Set(persistedReferences.map((reference) => reference.reference));
  const detached = previous.certificate_references.filter((reference) =>
    !reachableIds.has(reference.id)
    && !persistedIds.has(reference.id)
    && !persistedHandles.has(reference.reference),
  );
  if (detached.length === 0) return { workspace: next, detached };
  const detachedIds = new Set(detached.map((reference) => reference.id));
  return {
    workspace: {
      ...next,
      certificate_references: next.certificate_references.filter(
        (reference) => !detachedIds.has(reference.id),
      ),
    },
    detached,
  };
}
