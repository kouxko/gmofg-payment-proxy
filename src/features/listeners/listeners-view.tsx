"use client";

import { useState } from "react";
import { Alert, Button, Chip, Spinner, Table, toast } from "@heroui/react";
import type {
  ListenerCertificateDetailViewModel,
  ListenerCertificateImportViewModel,
  ListenerStatusViewModel,
  ListenerUpstreamTlsTestViewModel,
  ProxyListener,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
  WorkspaceValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ListenerEditor } from "./listener-editor";

type Pending = "validate" | "save" | "start" | "stop" | "secret" | "tls-test" | "import-identity" | "import-trust";

export function ListenersView() {
  const { navigate } = useWorkspaceNavigation();
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>("listener-workspaces", () => callCommand(commands.workspaceList()));
  const currentId = workspaces.data?.find((item) => item.selected)?.id ?? workspaces.data?.[0]?.id;
  const workspaceQuery = useIpcQuery<ProxyWorkspace>(
    `listener-workspace:${currentId ?? "none"}`,
    () => callCommand(commands.workspaceGet(currentId!)),
    undefined,
    { enabled: Boolean(currentId) },
  );
  const statuses = useIpcQuery<ListenerStatusViewModel[]>("listener-statuses", () => callCommand(commands.listenerStatuses()), []);
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
  const [startBlockedMessage, setStartBlockedMessage] = useState<string>();

  const effectiveWorkspace = workspace?.id === currentId ? workspace : workspaceQuery.data;
  const effectiveIndex = Math.min(selectedIndex, Math.max(0, (effectiveWorkspace?.listeners.length ?? 1) - 1));
  const selected = effectiveWorkspace?.listeners[effectiveIndex];
  const selectedStatus = statuses.data?.find((status) => status.listener_id === selected?.id);
  const selectedIsRunning = selectedStatus?.state === "running" || selectedStatus?.state === "starting";
  const hasUnsavedChanges = workspace !== undefined
    && workspace.id === currentId
    && workspaceQuery.data !== undefined
    && !sameWorkspace(workspace, workspaceQuery.data);
  const effectiveCertificateDetails = mergeCertificateDetails(
    certificateDetails.data ?? [],
    importedCertificateDetails,
  );

  function clearDerivedResults() {
    setValidation(undefined);
    setTlsTest(undefined);
    setTlsTestError(undefined);
    setStartBlockedMessage(undefined);
  }

  function replaceSelected(changes: Partial<ProxyListener>) {
    if (!effectiveWorkspace || !selected) return;
    setWorkspace({
      ...effectiveWorkspace,
      listeners: effectiveWorkspace.listeners.map((listener, index) =>
        index === effectiveIndex ? { ...listener, ...changes } : listener,
      ),
    });
    clearDerivedResults();
  }

  async function validateWorkspace() {
    if (!effectiveWorkspace || pending) return;
    await withPending("validate", async () => {
      const result = await callCommand(commands.workspaceValidate(effectiveWorkspace));
      setValidation(result);
      if (result.valid) toast("Rust 校验通过。", { variant: "success" });
    });
  }

  async function saveWorkspace() {
    if (!effectiveWorkspace || pending) return;
    await withPending("save", async () => {
      const result = await callCommand(commands.workspaceValidate(effectiveWorkspace));
      setValidation(result);
      if (!result.valid) return;
      const saved = await callCommand(commands.workspaceSave(result.normalized));
      setWorkspace(saved);
      workspaceQuery.setData(saved);
      toast("代理监听已保存。", { variant: "success" });
      await workspaces.refresh();
    });
  }

  async function toggleListenerRuntime() {
    if (!effectiveWorkspace || !selected || pending) return;
    const operation = selectedIsRunning ? "stop" : "start";
    await withPending(operation, async () => {
      let revision = effectiveWorkspace.revision;
      if (!selectedIsRunning && hasUnsavedChanges) {
        const otherListenerIsActive = statuses.data?.some((status) =>
          status.listener_id !== selected.id
          && (status.state === "running" || status.state === "starting"),
        );
        if (otherListenerIsActive) {
          const message = "当前监听配置尚未保存，且已有其他监听正在运行。请先停止运行中的监听，再保存当前修改后启动。";
          setStartBlockedMessage(message);
          toast(message, { variant: "danger" });
          return;
        }
        const result = await callCommand(commands.workspaceValidate(effectiveWorkspace));
        setValidation(result);
        if (!result.valid) return;
        const saved = await callCommand(commands.workspaceSave(result.normalized));
        revision = saved.revision;
        setWorkspace(saved);
        workspaceQuery.setData(saved);
      }
      const status = selectedIsRunning
        ? await callCommand(commands.listenerStop(effectiveWorkspace.id, revision, selected.id))
        : await callCommand(commands.listenerStart(effectiveWorkspace.id, revision, selected.id));
      toast(`代理监听${status.state_text}。`, { variant: status.state === "faulted" ? "danger" : "success" });
      const refreshed = await callCommand(commands.workspaceGet(effectiveWorkspace.id));
      setWorkspace(refreshed);
      workspaceQuery.setData(refreshed);
      setStartBlockedMessage(undefined);
      statuses.setData((current) => [...(current ?? []).filter((item) => item.listener_id !== status.listener_id), status]);
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

  function removeSelectedListener() {
    if (!effectiveWorkspace || !selected || selectedIsRunning || pending) return;
    setWorkspace({ ...effectiveWorkspace, listeners: effectiveWorkspace.listeners.filter((_, index) => index !== effectiveIndex) });
    setSelectedIndex(Math.max(0, effectiveIndex - 1));
    clearDerivedResults();
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
      const result = await callCommand(commands.workspaceValidate(effectiveWorkspace));
      setValidation(result);
      if (!result.valid) return;
      const saved = await callCommand(commands.workspaceSave(result.normalized));
      setWorkspace(saved);
      workspaceQuery.setData(saved);
      const test = await callCommand(commands.listenerTestUpstreamTls(saved.id, selected.id));
      setTlsTest(test);
      toast(test.message, { variant: "success" });
      await workspaces.refresh();
    }, (reason) => setTlsTestError(errorMessage(reason)));
  }

  async function importIdentity(label: string, password: string) {
    return importCertificate("import-identity", async () => callCommand(commands.listenerImportUpstreamClientIdentity(label, password)), "client_identity");
  }

  async function importTrust(label: string) {
    return importCertificate("import-trust", async () => callCommand(commands.listenerImportUpstreamServerTrust(label)), "server_trust");
  }

  async function importCertificate(kind: "import-identity" | "import-trust", load: () => Promise<ListenerCertificateImportViewModel | null>, field: "client_identity" | "server_trust") {
    if (!effectiveWorkspace || !selected?.fixed_server || pending) return false;
    let importedSuccessfully = false;
    await withPending(kind, async () => {
      const result = await load();
      if (!result || !selected.fixed_server) return;
      const { reference, detail } = result;
      replaceSelected({
        fixed_server: {
          ...selected.fixed_server,
          upstream_tls: { ...selected.fixed_server.upstream_tls, [field]: reference.id },
        },
      });
      setWorkspace((current) => current ? {
        ...current,
        certificate_references: [...current.certificate_references.filter((item) => item.id !== reference.id), reference],
      } : current);
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
          <div className="ml-auto flex flex-wrap gap-2"><Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void copySelectedListener()}>复制监听</Button><Button variant="danger-soft" isDisabled={!selected || selectedIsRunning || Boolean(pending)} onPress={removeSelectedListener}>删除监听</Button><Button variant="outline" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void validateWorkspace()}>{pending === "validate" ? "校验中…" : "Rust 校验"}</Button><Button variant="primary" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void saveWorkspace()}>{pending === "save" ? "保存中…" : "校验并保存"}</Button></div>
        </div>
        {validation && (validation.valid ? <Alert status="success"><Alert.Indicator /><Alert.Content><Alert.Title>Rust 校验通过</Alert.Title><Alert.Description>当前 Workspace 可保存。</Alert.Description></Alert.Content></Alert> : <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>Rust 校验未通过</Alert.Title><Alert.Description>{errors.map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description></Alert.Content></Alert>)}
        {certificateDetails.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>证书详情读取失败</Alert.Title><Alert.Description>{certificateDetails.error}</Alert.Description></Alert.Content></Alert>}
        {startBlockedMessage && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>无法启动当前监听</Alert.Title><Alert.Description>{startBlockedMessage}</Alert.Description></Alert.Content></Alert>}
        {!selected ? <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">选择一个代理监听进行编辑。</p> : <><div className="flex items-center justify-between rounded-2xl border border-[var(--telemetry-line)] p-3"><span className="text-sm">运行状态：{selectedStatus?.state_text ?? "已停止"}</span><Button variant={selectedIsRunning ? "danger-soft" : "primary"} isDisabled={Boolean(pending) || selectedStatus?.state === "starting" || selectedStatus?.state === "stopping"} onPress={() => void toggleListenerRuntime()}>{pending === "start" ? "启动中…" : pending === "stop" ? "停止中…" : selectedIsRunning ? "停止监听" : "启动监听"}</Button></div><ListenerEditor listener={selected} certificateReferences={effectiveWorkspace.certificate_references} certificateDetails={effectiveCertificateDetails} pending={pending} tlsTest={tlsTest} tlsTestError={tlsTestError} basicUsername={basicUsername} basicPassword={basicPassword} onBasicUsernameChange={setBasicUsername} onBasicPasswordChange={setBasicPassword} onChange={replaceSelected} onStoreBasicCredential={storeBasicCredential} onImportClientIdentity={importIdentity} onImportServerTrust={importTrust} onTestUpstreamTls={testUpstreamTls} /></>}
      </main>
    </section>
  );
}

function ListenerTable({ listeners, selectedIndex, onSelect }: { listeners: ProxyListener[]; selectedIndex: number; onSelect: (index: number) => void }) {
  return <Table><Table.ScrollContainer><Table.Content aria-label="代理监听列表"><Table.Header><Table.Column isRowHeader>监听名称</Table.Column><Table.Column>客户端连接 → 请求去向</Table.Column></Table.Header><Table.Body renderEmptyState={() => <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">当前工作区还没有代理监听</div>}>{listeners.map((listener, index) => <Table.Row key={listener.id} id={listener.id} onAction={() => onSelect(index)} className={index === selectedIndex ? "bg-[var(--telemetry-accent-soft)]" : ""}><Table.Cell><span className="font-medium">{listener.name}</span></Table.Cell><Table.Cell><div className="grid min-w-0 gap-1 font-mono text-xs"><span className="truncate">{listener.bind_address}:{listener.port}</span><span className="truncate text-[var(--telemetry-muted)]">→ {listener.fixed_server?.upstream_url || "请求中的目标地址"}</span></div></Table.Cell></Table.Row>)}</Table.Body></Table.Content></Table.ScrollContainer></Table>;
}

function sameWorkspace(left: ProxyWorkspace, right: ProxyWorkspace) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function mergeCertificateDetails(
  first: ListenerCertificateDetailViewModel[],
  second: ListenerCertificateDetailViewModel[],
) {
  const details = new Map(first.map((detail) => [detail.reference_id, detail]));
  for (const detail of second) details.set(detail.reference_id, detail);
  return [...details.values()];
}
