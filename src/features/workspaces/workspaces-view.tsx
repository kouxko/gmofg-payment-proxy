"use client";

import { useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Card,
  Chip,
  Input,
  Label,
  Spinner,
  Table,
  toast,
} from "@heroui/react";
import { ArrowDownToLine, ArrowUpFromLine, Copy, Plus, TrashBin } from "@gravity-ui/icons";
import type { ProxyWorkspace, WorkspaceSummaryViewModel } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { WorkspaceComponentsEditor } from "./workspace-components-editor";

export function WorkspacesView() {
  const list = useIpcQuery<WorkspaceSummaryViewModel[]>("workspace-list", () =>
    callCommand(commands.workspaceList()),
  );
  const [selectedId, setSelectedId] = useState<string>();
  const [draft, setDraft] = useState<ProxyWorkspace>();
  const [newName, setNewName] = useState("");
  const [pendingAction, setPendingAction] = useState<string>();
  const [deleteOpen, setDeleteOpen] = useState(false);
  const effectiveSelectedId = selectedId ?? list.data?.find((item) => item.selected)?.id ?? list.data?.[0]?.id;
  const selectedSummary = list.data?.find((item) => item.id === effectiveSelectedId);
  const detail = useIpcQuery<ProxyWorkspace>(
    `workspace:${effectiveSelectedId ?? "none"}`,
    () => callCommand(commands.workspaceGet(effectiveSelectedId!)),
    undefined,
    { enabled: Boolean(effectiveSelectedId) },
  );

  const effectiveDraft = draft?.id === effectiveSelectedId ? draft : detail.data;

  async function run(action: string, task: () => Promise<void>) {
    if (pendingAction) return;
    setPendingAction(action);
    try {
      await task();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingAction(undefined);
    }
  }

  function reload(selected?: string) {
    if (selected) setSelectedId(selected);
    void list.refresh();
    void detail.refresh();
  }

  async function createWorkspace() {
    const created = await callCommand(commands.workspaceCreate(newName));
    setNewName("");
    setSelectedId(created.id);
    await list.refresh();
  }

  async function saveWorkspace() {
    if (!effectiveDraft) return;
    const validation = await callCommand(commands.workspaceValidate(effectiveDraft));
    if (!validation.valid) {
      toast(Object.values(validation.field_errors).flat().join("；") || "Workspace 校验失败。", { variant: "danger" });
      return;
    }
    const saved = await callCommand(commands.workspaceSave(validation.normalized));
    setDraft(saved);
    toast("Workspace 已保存。", { variant: "success" });
    reload(saved.id);
  }

  async function addComponent(kind: Parameters<typeof commands.workspaceComponentNew>[1]) {
    if (!effectiveDraft) return;
    const updated = await callCommand(commands.workspaceComponentNew(effectiveDraft, kind));
    setDraft(updated);
  }

  async function applyComponentIntent(
    componentKind: string,
    componentId: string,
    operation: string,
    value: string,
  ) {
    if (!effectiveDraft) return;
    const updated = await callCommand(
      commands.workspaceComponentApplyIntent(
        effectiveDraft,
        componentKind,
        componentId,
        operation,
        value,
      ),
    );
    setDraft(updated);
  }

  return (
    <section className="grid h-full grid-cols-[minmax(420px,1fr)_380px] max-[1000px]:grid-cols-1">
      <div className="min-w-0 space-y-4 overflow-auto p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-64">
            <h1 className="text-2xl font-semibold">Workspace</h1>
            <p className="mt-1 text-sm text-[var(--telemetry-muted)]">列表、导入导出、复制、选择与持久化均由 Rust 执行。</p>
          </div>
          <div className="flex min-w-0 flex-wrap items-center justify-end gap-2 max-[720px]:w-full max-[720px]:justify-start">
            <Input
              aria-label="新 Workspace 名称"
              className="w-72 max-[720px]:min-w-56 max-[720px]:flex-1"
              value={newName}
              onChange={(event) => setNewName(event.target.value)}
              placeholder="新 Workspace 名称"
            />
            <Button variant="primary" isDisabled={Boolean(pendingAction)} onPress={() => void run("create", createWorkspace)}>
              <Plus className="size-4" />新建
            </Button>
            <Button variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("import", async () => {
              const result = await callCommand(commands.workspaceImport());
              toast(result.message, { variant: result.cancelled ? "default" : "success" });
              await list.refresh();
            })}>
              <ArrowUpFromLine className="size-4" />导入
            </Button>
          </div>
        </div>
        {list.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>读取 Workspace 失败</Alert.Title><Alert.Description>{list.error}</Alert.Description></Alert.Content><Button size="sm" variant="outline" onPress={() => void list.refresh()}>重试</Button></Alert>}
        <Table>
          <Table.ScrollContainer>
            <Table.Content aria-label="Workspace 列表">
              <Table.Header>
                <Table.Column isRowHeader>名称</Table.Column><Table.Column>代理入口</Table.Column><Table.Column>已启用入口</Table.Column><Table.Column>版本</Table.Column><Table.Column>状态</Table.Column>
              </Table.Header>
              <Table.Body renderEmptyState={() => <div className="p-8 text-center text-sm text-[var(--telemetry-muted)]">暂无 Workspace</div>}>
                {(list.data ?? []).map((item) => (
                  <Table.Row key={item.id} id={item.id} onAction={() => { setSelectedId(item.id); setDraft(undefined); }} className={item.id === effectiveSelectedId ? "bg-[var(--telemetry-accent-soft)]" : ""}>
                    <Table.Cell>{item.name}</Table.Cell><Table.Cell>{item.listener_count}</Table.Cell><Table.Cell>{item.enabled_listener_count}</Table.Cell><Table.Cell>{item.revision}</Table.Cell>
                    <Table.Cell>{item.selected ? <Chip color="success" variant="soft" size="sm">当前</Chip> : "—"}</Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
        {list.isLoading && <Spinner aria-label="正在读取 Workspace" />}
        {effectiveDraft && (
          <Card>
            <Card.Content className="p-4">
              <div className="mb-4">
                <Card.Title>Workspace 策略与安全引用</Card.Title>
                <Card.Description>组件 ID 由 Rust 创建；最终校验、执行和持久化也全部在 Rust。</Card.Description>
              </div>
              <WorkspaceComponentsEditor
                workspace={effectiveDraft}
                onChange={setDraft}
                onAdd={(kind) => void run(`add-${kind}`, () => addComponent(kind))}
                onIntent={(kind, id, operation, value) =>
                  void run(
                    `${operation}-${id}`,
                    () => applyComponentIntent(kind, id, operation, value),
                  )
                }
                disabled={Boolean(pendingAction)}
              />
            </Card.Content>
          </Card>
        )}
      </div>
      <aside className="min-w-0 space-y-4 overflow-auto border-l border-[var(--telemetry-line)] p-5 max-[1000px]:border-l-0 max-[1000px]:border-t">
        <h2 className="text-lg font-semibold">所选 Workspace</h2>
        {detail.isLoading ? <Spinner aria-label="正在读取 Workspace 详情" /> : effectiveDraft ? (
          <>
            <div className="grid gap-1"><Label>名称</Label><Input aria-label="Workspace 名称" value={effectiveDraft.name} onChange={(event) => setDraft({ ...effectiveDraft, name: event.target.value })} /></div>
            <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 text-sm"><dt>ID</dt><dd className="break-all font-mono text-xs">{effectiveDraft.id}</dd><dt>代理入口</dt><dd>{effectiveDraft.listeners.length}</dd><dt>版本</dt><dd>{effectiveDraft.revision}</dd></dl>
            <Button fullWidth variant="primary" isDisabled={Boolean(pendingAction)} onPress={() => void run("save", saveWorkspace)}>保存</Button>
            <Button fullWidth variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("select", async () => { await callCommand(commands.workspaceSelect(effectiveDraft.id)); toast("已切换当前 Workspace。", { variant: "success" }); await list.refresh(); })}>设为当前 Workspace</Button>
            <Button fullWidth variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("copy", async () => { const copied = await callCommand(commands.workspaceCopy(effectiveDraft.id)); setSelectedId(copied.id); setDraft(copied); await list.refresh(); })}><Copy className="size-4" />复制</Button>
            <Button fullWidth variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("export", async () => { const result = await callCommand(commands.workspaceExport(effectiveDraft.id)); toast(result.message, { variant: result.cancelled ? "default" : "success" }); })}><ArrowDownToLine className="size-4" />导出</Button>
            <AlertDialog isOpen={deleteOpen} onOpenChange={setDeleteOpen}>
              <Button fullWidth variant="danger-soft"><TrashBin className="size-4" />删除</Button>
              <AlertDialog.Backdrop><AlertDialog.Container><AlertDialog.Dialog>
                <AlertDialog.Header><AlertDialog.Heading>删除 {selectedSummary?.name ?? effectiveDraft.name}？</AlertDialog.Heading></AlertDialog.Header>
                <AlertDialog.Body>此操作会删除 Rust 存储中的 Workspace。</AlertDialog.Body>
                <AlertDialog.Footer><Button slot="close" variant="outline">取消</Button><Button variant="danger" onPress={() => void run("delete", async () => { const result = await callCommand(commands.workspaceDelete(effectiveDraft.id, effectiveDraft.revision)); toast(result.message, { variant: "success" }); setDeleteOpen(false); setSelectedId(undefined); setDraft(undefined); await list.refresh(); })}>确认删除</Button></AlertDialog.Footer>
              </AlertDialog.Dialog></AlertDialog.Container></AlertDialog.Backdrop>
            </AlertDialog>
          </>
        ) : <p className="text-sm text-[var(--telemetry-muted)]">选择一个 Workspace 查看详情。</p>}
      </aside>
    </section>
  );
}
