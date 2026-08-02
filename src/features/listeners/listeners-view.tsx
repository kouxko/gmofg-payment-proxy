"use client";

import { useState } from "react";
import {
  Alert,
  Button,
  Card,
  Chip,
  Input,
  Label,
  NumberField,
  ListBox,
  Select,
  Spinner,
  Switch,
  Table,
  toast,
} from "@heroui/react";
import type {
  ForwardProxyListener,
  ListenerStatusViewModel,
  ListenerUpstreamTlsTestViewModel,
  ProxyWorkspace,
  ReverseProxyListener,
  WorkspaceSummaryViewModel,
  WorkspaceValidationViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";

export function ListenersView() {
  const { navigate } = useWorkspaceNavigation();
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>("listener-workspaces", () => callCommand(commands.workspaceList()));
  const currentId = workspaces.data?.find((item) => item.selected)?.id ?? workspaces.data?.[0]?.id;
  const workspaceQuery = useIpcQuery<ProxyWorkspace>(`listener-workspace:${currentId ?? "none"}`, () => callCommand(commands.workspaceGet(currentId!)), undefined, { enabled: Boolean(currentId) });
  const statuses = useIpcQuery<ListenerStatusViewModel[]>("listener-statuses", () => callCommand(commands.listenerStatuses()), []);
  const [workspace, setWorkspace] = useState<ProxyWorkspace>();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [validation, setValidation] = useState<WorkspaceValidationViewModel>();
  const [pending, setPending] = useState<"validate" | "save" | "start" | "stop" | "secret" | "tls-test">();
  const [tlsTest, setTlsTest] = useState<ListenerUpstreamTlsTestViewModel>();
  const [tlsTestError, setTlsTestError] = useState<string>();
  const [basicUsername, setBasicUsername] = useState("");
  const [basicPassword, setBasicPassword] = useState("");

  const effectiveWorkspace = workspace?.id === currentId ? workspace : workspaceQuery.data;
  const effectiveIndex = Math.min(selectedIndex, Math.max(0, (effectiveWorkspace?.listeners.length ?? 1) - 1));
  const selected = effectiveWorkspace?.listeners[effectiveIndex];
  const selectedStatus = statuses.data?.find((status) => status.listener_id === selected?.id);
  const selectedIsRunning = selectedStatus?.state === "running" || selectedStatus?.state === "starting";
  const basicCredential = selected?.kind === "forward" && selected.authentication.mode === "basic" ? selected.authentication.credential : undefined;
  const downstreamClientAuthentication = selected?.kind === "reverse" ? selected.downstream_tls.client_authentication : undefined;

  function replaceForward(changes: Partial<ForwardProxyListener>) {
    if (!effectiveWorkspace || selected?.kind !== "forward") return;
    const listeners = effectiveWorkspace.listeners.map((listener, index) => index === effectiveIndex && listener.kind === "forward" ? { ...listener, ...changes } : listener);
    setWorkspace({ ...effectiveWorkspace, listeners });
    setValidation(undefined);
    setTlsTest(undefined);
    setTlsTestError(undefined);
  }

  function replaceReverse(changes: Partial<ReverseProxyListener>) {
    if (!effectiveWorkspace || selected?.kind !== "reverse") return;
    const listeners = effectiveWorkspace.listeners.map((listener, index) => index === effectiveIndex && listener.kind === "reverse" ? { ...listener, ...changes } : listener);
    setWorkspace({ ...effectiveWorkspace, listeners });
    setValidation(undefined);
    // 握手结果只对应测试时保存的入口快照。上游地址、CA、客户端身份或主机名策略
    // 任一编辑后都不能继续展示旧结果，否则会让用户误以为新配置已验证通过。
    setTlsTest(undefined);
    setTlsTestError(undefined);
  }

  async function validateWorkspace() {
    if (!effectiveWorkspace || pending) return;
    setPending("validate");
    try {
      const result = await callCommand(commands.workspaceValidate(effectiveWorkspace));
      setValidation(result);
      if (result.valid) toast("Rust 校验通过。", { variant: "success" });
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  async function saveWorkspace() {
    if (!effectiveWorkspace || pending) return;
    setPending("save");
    try {
      const result = await callCommand(commands.workspaceValidate(effectiveWorkspace));
      setValidation(result);
      if (!result.valid) return;
      const saved = await callCommand(commands.workspaceSave(result.normalized));
      setWorkspace(saved);
      toast("代理入口已保存。", { variant: "success" });
      await workspaces.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  async function toggleListenerRuntime() {
    if (!effectiveWorkspace || !selected || pending) return;
    const operation = selectedIsRunning ? "stop" : "start";
    setPending(operation);
    try {
      let workspaceRevision = effectiveWorkspace.revision;
      if (!selectedIsRunning) {
        const validationResult = await callCommand(commands.workspaceValidate(effectiveWorkspace));
        setValidation(validationResult);
        if (!validationResult.valid) return;
        const saved = await callCommand(commands.workspaceSave(validationResult.normalized));
        workspaceRevision = saved.revision;
        setWorkspace(saved);
      }
      const status = selectedIsRunning
        ? await callCommand(commands.listenerStop(effectiveWorkspace.id, workspaceRevision, selected.id))
        : await callCommand(commands.listenerStart(effectiveWorkspace.id, workspaceRevision, selected.id));
      toast(`代理入口${status.state_text}。`, { variant: status.state === "faulted" ? "danger" : "success" });
      const refreshed = await callCommand(commands.workspaceGet(effectiveWorkspace.id));
      setWorkspace(refreshed);
      statuses.setData((current) => [
        ...(current ?? []).filter((item) => item.listener_id !== status.listener_id),
        status,
      ]);
      await workspaces.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
      await statuses.refresh();
    } finally {
      setPending(undefined);
    }
  }

  async function addListener(kind: "forward" | "reverse") {
    if (!effectiveWorkspace || pending) return;
    setPending("save");
    try {
      const draft = await callCommand(commands.listenerNew(kind));
      setWorkspace({
        ...effectiveWorkspace,
        listeners: [...effectiveWorkspace.listeners, draft],
      });
      setSelectedIndex(effectiveWorkspace.listeners.length);
      setValidation(undefined);
      setTlsTest(undefined);
      setTlsTestError(undefined);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  async function copySelectedListener() {
    if (!effectiveWorkspace || !selected || pending) return;
    setPending("save");
    try {
      const draft = await callCommand(commands.listenerCopy(selected));
      setWorkspace({
        ...effectiveWorkspace,
        listeners: [...effectiveWorkspace.listeners, draft],
      });
      setSelectedIndex(effectiveWorkspace.listeners.length);
      setValidation(undefined);
      setTlsTest(undefined);
      setTlsTestError(undefined);
      toast("已创建独立监听映射副本，请修改本地端口和上游 Origin。", { variant: "success" });
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  function removeSelectedListener() {
    if (!effectiveWorkspace || !selected || selectedIsRunning || pending) return;
    const listeners = effectiveWorkspace.listeners.filter((_, index) => index !== effectiveIndex);
    setWorkspace({ ...effectiveWorkspace, listeners });
    setSelectedIndex(Math.max(0, effectiveIndex - 1));
    setValidation(undefined);
    setTlsTest(undefined);
    setTlsTestError(undefined);
  }

  async function storeBasicCredential() {
    if (selected?.kind !== "forward" || pending) return;
    setPending("secret");
    try {
      const credential = await callCommand(commands.workspaceSecretStoreBasic(basicUsername, basicPassword));
      replaceForward({ authentication: { mode: "basic", credential } });
      setBasicPassword("");
      toast("认证凭据已由系统密钥保护。", { variant: "success" });
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  async function testUpstreamTls() {
    if (!effectiveWorkspace || selected?.kind !== "reverse" || pending) return;
    setPending("tls-test");
    setTlsTest(undefined);
    setTlsTestError(undefined);
    try {
      // 测试必须使用与实际启动完全相同的持久化快照；先由 Rust 校验并保存当前表单，
      // 再让 Rust 根据安全引用读取 CA、客户端身份并真实连接 Server。
      const validationResult = await callCommand(commands.workspaceValidate(effectiveWorkspace));
      setValidation(validationResult);
      if (!validationResult.valid) return;
      const saved = await callCommand(commands.workspaceSave(validationResult.normalized));
      setWorkspace(saved);
      const result = await callCommand(commands.listenerTestUpstreamTls(saved.id, selected.id));
      setTlsTest(result);
      toast(result.message, { variant: "success" });
      await workspaces.refresh();
    } catch (reason) {
      const message = errorMessage(reason);
      setTlsTestError(message);
      toast(message, { variant: "danger" });
    } finally {
      setPending(undefined);
    }
  }

  const errors = Object.entries(validation?.field_errors ?? {});

  return (
    <section className="grid h-full grid-cols-[440px_minmax(0,1fr)] max-[1050px]:grid-cols-[360px_minmax(0,1fr)] max-[900px]:grid-cols-1">
      <aside className="min-w-0 space-y-4 overflow-auto border-r border-[var(--telemetry-line)] p-5 max-[900px]:border-r-0 max-[900px]:border-b">
        <div>
          <h1 className="text-2xl font-semibold">代理入口</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">决定客户端连接本机的哪个地址和端口，以及收到请求后转发到哪里。</p>
        </div>
        <div className="grid grid-cols-2 gap-2">
          <Button variant="outline" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void addListener("forward")}>新增正向代理</Button>
          <Button variant="primary" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void addListener("reverse")}>新增固定上游入口</Button>
        </div>
        <Alert status="accent">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>两种入口的区别</Alert.Title>
            <Alert.Description>正向代理可访问请求指定的任意目标；固定上游入口会把指定本地端口收到的请求始终转发到它自己的上游地址。</Alert.Description>
          </Alert.Content>
        </Alert>
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>入口只负责接收和转发流量</Alert.Title>
            <Alert.Description>先新增并启动入口，再到“故障模拟”选择快速模板，或到“拦截规则”配置精确条件与动作。模拟最终会作为规则作用于经过入口的请求。</Alert.Description>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button size="sm" variant="primary" onPress={() => navigate("/faults")}>去添加故障模拟</Button>
              <Button size="sm" variant="outline" onPress={() => navigate("/rules")}>去配置拦截规则</Button>
            </div>
          </Alert.Content>
        </Alert>
        {workspaceQuery.isLoading && <Spinner aria-label="正在读取代理入口" />}
        {workspaceQuery.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>读取失败</Alert.Title><Alert.Description>{workspaceQuery.error}</Alert.Description></Alert.Content></Alert>}
        <Table>
          <Table.ScrollContainer>
            <Table.Content aria-label="代理入口列表">
              <Table.Header><Table.Column isRowHeader>入口名称</Table.Column><Table.Column>客户端连接 → 请求去向</Table.Column></Table.Header>
              <Table.Body renderEmptyState={() => <div className="p-6 text-center text-sm text-[var(--telemetry-muted)]">当前工作区还没有代理入口</div>}>
                {(effectiveWorkspace?.listeners ?? []).map((listener, index) => (
                  <Table.Row key={listener.id} id={listener.id} onAction={() => { setSelectedIndex(index); setTlsTest(undefined); setTlsTestError(undefined); }} className={index === effectiveIndex ? "bg-[var(--telemetry-accent-soft)]" : ""}>
                    <Table.Cell>
                      <div className="grid gap-1">
                        <span className="font-medium">{listener.name}</span>
                        <span className="text-xs text-[var(--telemetry-muted)]">{listener.kind === "forward" ? "正向代理入口" : "固定上游入口"}</span>
                      </div>
                    </Table.Cell>
                    <Table.Cell>
                      <div className="grid min-w-0 gap-1 font-mono text-xs">
                        <span className="truncate">{listener.bind_address}:{listener.port}</span>
                        <span className="truncate text-[var(--telemetry-muted)]">→ {listener.kind === "reverse" ? (listener.upstream_url || "待填写上游 Origin") : "请求中的任意目标"}</span>
                      </div>
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
        <p className="text-xs text-[var(--telemetry-muted)]">例如可分别建立 `:18080 → upstream-a:18080` 与 `:18443 → upstream-b:18443`。每个入口可以使用不同的上游域名和端口。</p>
      </aside>
      <div className="min-w-0 space-y-5 overflow-auto p-5">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-xl font-semibold">{selected?.kind === "reverse" ? "固定上游入口配置" : "正向代理入口配置"}</h2>
          {selected && <Chip color="accent" variant="soft">{selected.kind === "forward" ? "正向代理入口" : "固定上游入口"}</Chip>}
          <div className="ml-auto flex gap-2">
            <Button variant="outline" isDisabled={!selected || Boolean(pending)} onPress={() => void copySelectedListener()}>复制为新入口</Button>
            <Button variant="danger-soft" isDisabled={!selected || selectedIsRunning || Boolean(pending)} onPress={removeSelectedListener}>删除入口</Button>
            <Button variant="outline" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void validateWorkspace()}>{pending === "validate" ? "校验中…" : "Rust 校验"}</Button>
            <Button variant="primary" isDisabled={!effectiveWorkspace || Boolean(pending)} onPress={() => void saveWorkspace()}>{pending === "save" ? "保存中…" : "校验并保存"}</Button>
          </div>
        </div>
        {validation && (validation.valid ? <Alert status="success"><Alert.Indicator /><Alert.Content><Alert.Title>Rust 校验通过</Alert.Title><Alert.Description>当前 Workspace 可保存。</Alert.Description></Alert.Content></Alert> : <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>Rust 校验未通过</Alert.Title><Alert.Description>{errors.map(([field, messages]) => `${field}: ${messages.join("，")}`).join("；")}</Alert.Description></Alert.Content></Alert>)}
        {!selected ? <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">选择一个代理入口进行编辑。</p> : (
          <Card>
            <Card.Content className="grid grid-cols-2 gap-4 p-5 max-[700px]:grid-cols-1">
              <div className="col-span-2 flex items-center justify-between max-[700px]:col-span-1">
                <div><Card.Title>{selected.name}</Card.Title><Card.Description className="font-mono text-xs">{selected.id}</Card.Description></div>
                <div className="flex items-center gap-2">
                  <Chip variant="soft">{selectedStatus?.state_text ?? "已停止"}</Chip>
                  <Button
                    variant={selectedIsRunning ? "danger-soft" : "primary"}
                    isDisabled={Boolean(pending) || selectedStatus?.state === "starting" || selectedStatus?.state === "stopping"}
                    onPress={() => void toggleListenerRuntime()}
                  >
                    {pending === "start" ? "启动中…" : pending === "stop" ? "停止中…" : selectedIsRunning ? "停止" : "校验、保存并启动"}
                  </Button>
                </div>
              </div>
              <div className="grid gap-1"><Label>入口名称</Label><Input aria-label="代理入口名称" value={selected.name} onChange={(event) => selected.kind === "forward" ? replaceForward({ name: event.target.value }) : replaceReverse({ name: event.target.value })} /></div>
              <div className="grid gap-1"><Label>绑定地址</Label><Input aria-label="绑定地址" value={selected.bind_address} onChange={(event) => selected.kind === "forward" ? replaceForward({ bind_address: event.target.value }) : replaceReverse({ bind_address: event.target.value })} /></div>
              <NumberField aria-label="监听端口" minValue={0} maxValue={65535} value={selected.port} onChange={(port) => selected.kind === "forward" ? replaceForward({ port }) : replaceReverse({ port })}><Label>监听端口</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
              {selected.kind === "reverse" ? (
                <div className="grid gap-1"><Label>固定上游 Origin（本映射独立）</Label><Input aria-label="上游 URL" value={selected.upstream_url} onChange={(event) => replaceReverse({ upstream_url: event.target.value })} placeholder="https://api.example.test:443" /></div>
              ) : (
                <>
                  <NumberField aria-label="连接超时毫秒" minValue={0} value={selected.connect_timeout_ms} onChange={(connect_timeout_ms) => replaceForward({ connect_timeout_ms })}><Label>连接超时（ms）</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
                  <NumberField aria-label="读取超时毫秒" minValue={0} value={selected.read_timeout_ms} onChange={(read_timeout_ms) => replaceForward({ read_timeout_ms })}><Label>读取超时（ms）</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
                  <NumberField aria-label="写入超时毫秒" minValue={0} value={selected.write_timeout_ms} onChange={(write_timeout_ms) => replaceForward({ write_timeout_ms })}><Label>写入超时（ms）</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
                  <div className="col-span-2 grid gap-1 max-[700px]:col-span-1"><Label>允许的客户端 CIDR</Label><Input aria-label="允许的客户端 CIDR" value={selected.allowed_client_cidrs.join(", ")} onChange={(event) => replaceForward({ allowed_client_cidrs: event.target.value.split(",").map((value) => value.trim()).filter(Boolean) })} placeholder="127.0.0.1/32, 10.0.0.0/8" /></div>
                  <div className="col-span-2 grid grid-cols-2 gap-4 rounded-2xl border border-[var(--telemetry-line)] p-4 max-[700px]:col-span-1 max-[700px]:grid-cols-1">
                    <Switch isSelected={selected.authentication.mode === "basic"} onChange={(enabled) => replaceForward({ authentication: enabled ? { mode: "basic", credential: { provider: "system", key: "" } } : { mode: "none" } })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>启用 HTTP Basic 认证</span></Switch.Content></Switch>
                    <Switch isSelected={selected.mitm.enabled} onChange={(enabled) => replaceForward({ mitm: { ...selected.mitm, enabled } })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>启用 allowlist MITM</span></Switch.Content></Switch>
                    {basicCredential && <>
                      <div className="grid gap-1"><Label>用户名</Label><Input aria-label="代理认证用户名" value={basicUsername} onChange={(event) => setBasicUsername(event.target.value)} autoComplete="off" /></div>
                      <div className="grid gap-1"><Label>密码</Label><Input aria-label="代理认证密码" type="password" value={basicPassword} onChange={(event) => setBasicPassword(event.target.value)} autoComplete="new-password" /></div>
                      <div className="col-span-2 flex items-center justify-between gap-3 max-[700px]:col-span-1">
                        <p className="min-w-0 truncate text-xs text-[var(--telemetry-muted)]">{basicCredential.key ? `已保存安全引用：${basicCredential.provider}/${basicCredential.key}` : "尚未保存凭据；明文不会写入 Workspace。"}</p>
                        <Button variant="outline" isDisabled={!basicUsername || !basicPassword || Boolean(pending)} onPress={() => void storeBasicCredential()}>{pending === "secret" ? "保护中…" : "保护并引用"}</Button>
                      </div>
                    </>}
                    {selected.mitm.enabled && <>
                      <div className="col-span-2 grid gap-1 max-[700px]:col-span-1"><Label>MITM authority allowlist</Label><Input aria-label="MITM authority allowlist" value={selected.mitm.authority_allowlist.join(", ")} onChange={(event) => replaceForward({ mitm: { ...selected.mitm, authority_allowlist: event.target.value.split(",").map((value) => value.trim()).filter(Boolean) } })} placeholder="api.example.test, *.test.example" /></div>
                      <NumberField aria-label="MITM 叶子证书缓存" minValue={1} maxValue={256} value={selected.mitm.maximum_cached_leaf_certificates} onChange={(maximum_cached_leaf_certificates) => replaceForward({ mitm: { ...selected.mitm, maximum_cached_leaf_certificates } })}><Label>叶子证书缓存</Label><NumberField.Group><NumberField.DecrementButton /><NumberField.Input /><NumberField.IncrementButton /></NumberField.Group></NumberField>
                      <p className="self-end text-xs text-[var(--telemetry-muted)]">Root CA 留空时使用当前安装实例的受保护 Root；只可导出公开证书。</p>
                    </>}
                  </div>
                </>
              )}
              {selected.kind === "reverse" && <div className="col-span-2 grid grid-cols-2 gap-4 rounded-2xl border border-[var(--telemetry-line)] p-4 max-[700px]:col-span-1 max-[700px]:grid-cols-1">
                <Switch isSelected={selected.downstream_tls.enabled} onChange={(enabled) => replaceReverse({ downstream_tls: { ...selected.downstream_tls, enabled } })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>下游 TLS / mTLS</span></Switch.Content></Switch>
                <Switch isSelected={selected.upstream_tls.verify_hostname} onChange={(verify_hostname) => replaceReverse({ upstream_tls: { ...selected.upstream_tls, verify_hostname } })}><Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><span>校验上游主机名</span></Switch.Content></Switch>
                {selected.downstream_tls.enabled && <>
                  <div className="grid gap-1"><Label>服务端身份引用 ID</Label><Input aria-label="下游服务端身份引用" value={selected.downstream_tls.server_identity ?? ""} onChange={(event) => replaceReverse({ downstream_tls: { ...selected.downstream_tls, server_identity: event.target.value || null } })} /></div>
                  <Select
                    aria-label="下游客户端认证模式"
                    selectedKey={selected.downstream_tls.client_authentication.mode}
                    onSelectionChange={(key) => {
                      const mode = String(key);
                      replaceReverse({
                        downstream_tls: {
                          ...selected.downstream_tls,
                          client_authentication:
                            mode === "required"
                              ? { mode: "required", trust: "" }
                              : mode === "optional"
                                ? { mode: "optional", trust: "" }
                                : { mode: "disabled" },
                        },
                      });
                    }}
                  >
                    <Label>客户端认证模式</Label>
                    <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
                    <Select.Popover><ListBox>
                      <ListBox.Item id="disabled" textValue="不校验客户端证书">不校验客户端证书</ListBox.Item>
                      <ListBox.Item id="optional" textValue="可选客户端证书">可选客户端证书</ListBox.Item>
                      <ListBox.Item id="required" textValue="必须提供客户端证书">必须提供客户端证书</ListBox.Item>
                    </ListBox></Select.Popover>
                  </Select>
                  {downstreamClientAuthentication && downstreamClientAuthentication.mode !== "disabled" && <div className="col-span-2 grid gap-1 max-[700px]:col-span-1"><Label>下游客户端 CA 引用 ID</Label><Input aria-label="下游客户端 CA 引用" value={downstreamClientAuthentication.trust} onChange={(event) => replaceReverse({ downstream_tls: { ...selected.downstream_tls, client_authentication: downstreamClientAuthentication.mode === "required" ? { mode: "required", trust: event.target.value } : { mode: "optional", trust: event.target.value } } })} /></div>}
                </>}
                <div className="grid gap-1"><Label>上游客户端身份引用 ID（可选）</Label><Input aria-label="上游客户端身份引用" value={selected.upstream_tls.client_identity ?? ""} onChange={(event) => replaceReverse({ upstream_tls: { ...selected.upstream_tls, client_identity: event.target.value || null } })} /></div>
                <div className="grid gap-1"><Label>上游 CA 引用 ID（可选）</Label><Input aria-label="上游 CA 引用" value={selected.upstream_tls.server_trust ?? ""} onChange={(event) => replaceReverse({ upstream_tls: { ...selected.upstream_tls, server_trust: event.target.value || null } })} /></div>
                <div className="grid gap-1"><Label>请求 Body Codec 引用 ID（可选）</Label><Input aria-label="请求 Body Codec 引用" value={selected.request_codec_policy ?? ""} onChange={(event) => replaceReverse({ request_codec_policy: event.target.value || null })} /></div>
                <div className="grid gap-1"><Label>响应 Body Codec 引用 ID（可选）</Label><Input aria-label="响应 Body Codec 引用" value={selected.response_codec_policy ?? ""} onChange={(event) => replaceReverse({ response_codec_policy: event.target.value || null })} /></div>
                <p className="col-span-2 text-xs text-[var(--telemetry-muted)] max-[700px]:col-span-1">引用 ID 来自 Workspace 的证书与 Codec 安全引用；密码和私钥不会进入前端或 Workspace 文档。</p>
                <div className="col-span-2 flex flex-wrap items-center gap-3 border-t border-[var(--telemetry-line)] pt-4 max-[700px]:col-span-1">
                  <Button variant="outline" isDisabled={Boolean(pending)} onPress={() => void testUpstreamTls()}>
                    {pending === "tls-test" ? "正在连接 Server…" : "测试上游 TLS 握手"}
                  </Button>
                  <p className="text-xs text-[var(--telemetry-muted)]">使用本入口的上游地址、CA、主机名策略和可选客户端身份进行真实握手，不发送 HTTP 业务请求。</p>
                </div>
                {tlsTest && <Alert status="success" className="col-span-2 max-[700px]:col-span-1">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>{tlsTest.message}</Alert.Title>
                    <Alert.Description>
                      <span className="grid gap-1">
                        <span>连接：{tlsTest.resolved_address} · {tlsTest.elapsed_millis} ms</span>
                        <span>协商：{tlsTest.tls_version} · {tlsTest.cipher_suite}</span>
                        <span>Server：{tlsTest.peer_subject}</span>
                        <span className="break-all font-mono text-xs">SHA-256：{tlsTest.peer_sha256_fingerprint}</span>
                        <span>主机名校验：{tlsTest.hostname_verification_enabled ? "已启用并通过" : "按入口配置关闭"} · 客户端身份：{tlsTest.client_identity_configured ? "已配置" : "未配置（普通 TLS）"}</span>
                      </span>
                    </Alert.Description>
                  </Alert.Content>
                </Alert>}
                {tlsTestError && <Alert status="danger" className="col-span-2 max-[700px]:col-span-1"><Alert.Indicator /><Alert.Content><Alert.Title>上游 TLS 握手失败</Alert.Title><Alert.Description>{tlsTestError}</Alert.Description></Alert.Content></Alert>}
              </div>}
            </Card.Content>
          </Card>
        )}
      </div>
    </section>
  );
}
