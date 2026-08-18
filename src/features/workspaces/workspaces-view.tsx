"use client";

import { useRef, useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Chip,
  Input,
  Label,
  Modal,
  Spinner,
  Table,
  toast,
} from "@heroui/react";
import { ArrowUpFromLine, Copy, Plus, TrashBin } from "@gravity-ui/icons";
import type {
  ApplicationBackupImportPreview,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";

export function WorkspacesView() {
  const list = useIpcQuery<WorkspaceSummaryViewModel[]>("workspace-list", () =>
    callCommand(commands.workspaceList()),
  );
  const [selectedId, setSelectedId] = useState<string>();
  const [draft, setDraft] = useState<ProxyWorkspace>();
  const [newName, setNewName] = useState("");
  const [pendingAction, setPendingAction] = useState<string>();
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [backupPreview, setBackupPreview] = useState<ApplicationBackupImportPreview>();
  const importRequest = useRef(0);
  const effectiveSelectedId = selectedId ?? list.data?.find((item) => item.selected)?.id ?? list.data?.[0]?.id;
  const selectedSummary = list.data?.find((item) => item.id === effectiveSelectedId);
  const detail = useIpcQuery<ProxyWorkspace>(
    `workspace:${effectiveSelectedId ?? "none"}`,
    () => callCommand(commands.workspaceGet(effectiveSelectedId!)),
    undefined,
    { enabled: Boolean(effectiveSelectedId) },
  );

  const effectiveDraft = draft?.id === effectiveSelectedId ? draft : detail.data;

  function refreshState(selected?: string) {
    if (selected) setSelectedId(selected);
    void list.refresh();
    void detail.refresh();
  }

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

  async function createWorkspace() {
    const created = await callCommand(commands.workspaceCreate(newName));
    setNewName("");
    setSelectedId(created.id);
    await list.refresh();
  }

  async function exportApplicationData() {
    const result = await callCommand(commands.applicationBackupExport());
    if (result) toast(`应用数据已导出（${result.bytes_written} 字节）。`, { variant: "success" });
  }

  async function prepareApplicationImport() {
    const request = ++importRequest.current;
    const preview = await callCommand(commands.applicationBackupImportPrepare());
    if (request !== importRequest.current || !preview) return;
    setBackupPreview(preview);
  }

  async function discardApplicationImport() {
    const preview = backupPreview;
    if (!preview) return;
    setBackupPreview(undefined);
    importRequest.current += 1;
    await callCommand(commands.applicationBackupImportDiscard(preview.token));
  }

  async function commitApplicationImport() {
    const preview = backupPreview;
    if (!preview) return;
    const outcome = await callCommand(commands.applicationBackupImportCommit(preview.token));
    setBackupPreview(undefined);
    setSelectedId(undefined);
    setDraft(undefined);
    toast(`已替换 ${outcome.workspace_count} 个 Workspace 和 ${outcome.protocol_package_count} 个协议包版本。`, { variant: "success" });
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
    refreshState(saved.id);
  }

  async function selectCurrentWorkspace() {
    if (!effectiveDraft) return;
    await callCommand(commands.workspaceSelect(effectiveDraft.id));
    toast("已切换当前 Workspace；运行中的代理入口和设备网络接管保持不变。", { variant: "success" });
    await list.refresh();
  }

  async function copyWorkspace() {
    if (!effectiveDraft) return;
    const copied = await callCommand(commands.workspaceCopy(effectiveDraft.id));
    setSelectedId(copied.id);
    setDraft(copied);
    await list.refresh();
  }

  async function deleteWorkspace() {
    if (!effectiveDraft) return;
    const result = await callCommand(commands.workspaceDelete(effectiveDraft.id, effectiveDraft.revision));
    toast(result.message, { variant: "success" });
    setDeleteOpen(false);
    setSelectedId(undefined);
    setDraft(undefined);
    await list.refresh();
  }

  return (
    <section className="grid h-full grid-cols-[minmax(420px,1fr)_380px] max-[1000px]:grid-cols-1">
      <div className="min-w-0 space-y-4 overflow-x-hidden overflow-y-auto p-5">
        <div className="flex min-w-0 items-center justify-between gap-3">
          <div className="min-w-64">
            <h1 className="sr-only">Workspace</h1>
            <p className="mt-1 text-sm text-[var(--telemetry-muted)]">在此创建、复制、选择及导入导出 Workspace。</p>
          </div>
          <div data-testid="workspace-toolbar" className="flex min-w-0 flex-nowrap items-center justify-end gap-2 overflow-x-auto overflow-y-hidden max-[720px]:w-full max-[720px]:justify-start">
            <Input
              aria-label="新 Workspace 名称"
              disabled={Boolean(pendingAction)}
              className="w-72 max-[720px]:min-w-56 max-[720px]:flex-1"
              value={newName}
              onChange={(event) => setNewName(event.target.value)}
              placeholder="新 Workspace 名称"
            />
            <Button variant="primary" isDisabled={Boolean(pendingAction)} onPress={() => void run("create", createWorkspace)}>
              <Plus className="size-4" />新建
            </Button>
            <Button variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("backup-export", exportApplicationData)}>
              导出应用数据
            </Button>
            <Button variant="danger-soft" isDisabled={Boolean(pendingAction)} onPress={() => void run("backup-prepare", prepareApplicationImport)}>
              <ArrowUpFromLine className="size-4" />导入应用数据
            </Button>
            <Modal
              isOpen={Boolean(backupPreview)}
              onOpenChange={(open) => {
                if (!open && backupPreview && !pendingAction) {
                  void run("backup-discard", discardApplicationImport);
                }
              }}
            >
              <Button className="hidden" aria-hidden="true">打开应用数据导入预览</Button>
              <Modal.Backdrop isDismissable={!pendingAction}><Modal.Container><Modal.Dialog>
                <Modal.Header><Modal.Heading>确认替换应用数据？</Modal.Heading></Modal.Header>
                <Modal.Body className="space-y-2">
                  <p>将替换全部 Workspace、当前选择、全局设置和协议包注册表。</p>
                  <p>{backupPreview?.workspace_count} 个 Workspace · {backupPreview?.protocol_package_count} 个协议包版本（启用 {backupPreview?.enabled_protocol_package_count}）· {backupPreview?.portable_material_count} 份证书材料</p>
                </Modal.Body>
                <Modal.Footer><Button slot="close" variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("backup-discard", discardApplicationImport)}>取消</Button><Button variant="danger" isDisabled={Boolean(pendingAction)} onPress={() => void run("backup-commit", commitApplicationImport)}>确认替换</Button></Modal.Footer>
              </Modal.Dialog></Modal.Container></Modal.Backdrop>
            </Modal>
          </div>
        </div>
        <Alert status="accent">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>切换只改变编辑上下文</Alert.Title>
            <Alert.Description>
              已运行的代理入口和设备网络接管不会自动停止，并继续使用启动时所属 Workspace 的配置。
            </Alert.Description>
          </Alert.Content>
        </Alert>
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
      </div>
      <aside className="min-w-0 space-y-4 overflow-auto border-l border-[var(--telemetry-line)] p-5 max-[1000px]:border-l-0 max-[1000px]:border-t">
        <h2 className="text-lg font-semibold">所选 Workspace</h2>
        {detail.isLoading ? <Spinner aria-label="正在读取 Workspace 详情" /> : effectiveDraft ? (
          <>
            <div className="grid gap-1"><Label>名称</Label><Input aria-label="Workspace 名称" disabled={Boolean(pendingAction)} value={effectiveDraft.name} onChange={(event) => setDraft({ ...effectiveDraft, name: event.target.value })} /></div>
            <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 text-sm"><dt>ID</dt><dd className="break-all font-mono text-xs">{effectiveDraft.id}</dd><dt>代理入口</dt><dd>{effectiveDraft.listeners.length}</dd><dt>版本</dt><dd>{effectiveDraft.revision}</dd></dl>
            <Button fullWidth variant="primary" isDisabled={Boolean(pendingAction)} onPress={() => void run("save", saveWorkspace)}>保存</Button>
            <Button fullWidth variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("select", selectCurrentWorkspace)}>设为当前 Workspace</Button>
            <Button fullWidth variant="outline" isDisabled={Boolean(pendingAction)} onPress={() => void run("copy", copyWorkspace)}><Copy className="size-4" />复制</Button>
            <AlertDialog isOpen={deleteOpen} onOpenChange={setDeleteOpen}>
              <Button fullWidth variant="danger-soft"><TrashBin className="size-4" />删除</Button>
              <AlertDialog.Backdrop><AlertDialog.Container><AlertDialog.Dialog>
                <AlertDialog.Header><AlertDialog.Heading>删除 {selectedSummary?.name ?? effectiveDraft.name}？</AlertDialog.Heading></AlertDialog.Header>
                <AlertDialog.Body>此操作会永久删除所选 Workspace。</AlertDialog.Body>
                <AlertDialog.Footer><Button slot="close" variant="outline">取消</Button><Button variant="danger" onPress={() => void run("delete", deleteWorkspace)}>确认删除</Button></AlertDialog.Footer>
              </AlertDialog.Dialog></AlertDialog.Container></AlertDialog.Backdrop>
            </AlertDialog>
          </>
        ) : <p className="text-sm text-[var(--telemetry-muted)]">选择一个 Workspace 查看详情。</p>}
      </aside>
    </section>
  );
}
