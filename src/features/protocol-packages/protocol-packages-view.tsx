"use client";

import { useRef, useState } from "react";
import { Alert, Button, Spinner, toast } from "@heroui/react";
import type { ProtocolPackageGroupViewModel, ProtocolPackageVersionViewModel } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ProtocolPackageDialog } from "./protocol-package-dialog";
import { ProtocolPackageImportDialog } from "./protocol-package-import-dialog";
import {
  importResultError,
  isCommittableImportPreview,
  isImportPreview,
  outcomeText,
  presentImportError,
  type CommittableImportPreview,
  type ProtocolPackageImportState,
  withoutImportToken,
} from "./protocol-package-import-model";
import { ProtocolPackageRow } from "./protocol-package-row";
import {
  ProtocolWorkspaceTabs,
  type ProtocolType,
} from "@/features/shared/protocol-workspace-tabs";
import {
  builtInRestoreResultError,
  isProtocolPackageGroupList,
  sortPackageVersions,
} from "./protocol-package-model";

export function ProtocolPackagesView() {
  const packages = useIpcQuery<ProtocolPackageGroupViewModel[]>(
    "protocol-packages",
    () => callCommand(commands.protocolPackageList()),
  );
  const [selectedGroup, setSelectedGroup] = useState<ProtocolPackageGroupViewModel>();
  const [selectedVersion, setSelectedVersion] = useState<ProtocolPackageVersionViewModel>();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [importState, setImportState] = useState<ProtocolPackageImportState>({ kind: "closed" });
  const [importNotice, setImportNotice] = useState<string>();
  const [restoreError, setRestoreError] = useState<string>();
  const [restorePending, setRestorePending] = useState(false);
  const [exportPending, setExportPending] = useState(false);
  const [selectedKind, setSelectedKind] = useState<ProtocolType>("socket");
  // state 更新发生在下一次渲染；ref 在事件入口同步上锁，阻止同一帧的重复点击。
  const prepareLock = useRef(false);
  const commitLock = useRef(false);
  const discardLock = useRef(false);
  const restoreLock = useRef(false);
  const exportLock = useRef(false);
  const importGeneration = useRef(0);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const importTriggerRef = useRef<HTMLButtonElement | null>(null);
  const headingRef = useRef<HTMLHeadingElement | null>(null);
  const listIsValid = isProtocolPackageGroupList(packages.data);
  const listError = packages.error
    ?? (!packages.isLoading && !listIsValid
      ? "协议包列表返回了不完整的数据。"
      : undefined);
  const groups: ProtocolPackageGroupViewModel[] = listIsValid && packages.data
    ? packages.data.filter((group) => group.kind === selectedKind)
    : [];

  function openGroup(group: ProtocolPackageGroupViewModel, trigger: HTMLButtonElement) {
    triggerRef.current = trigger;
    setImportNotice(undefined);
    setSelectedGroup(group);
    setSelectedVersion(sortPackageVersions(group.versions)[0]);
    setDialogOpen(true);
  }

  function changeOpen(open: boolean) {
    setDialogOpen(open);
    if (!open) {
      setImportNotice(undefined);
      requestAnimationFrame(() => {
        const focusTarget = triggerRef.current?.isConnected
          ? triggerRef.current
          : headingRef.current;
        focusTarget?.focus();
      });
    }
  }

  async function chooseZip() {
    if (prepareLock.current || commitLock.current || restoreLock.current || exportLock.current) return;
    triggerRef.current = importTriggerRef.current;
    const generation = importGeneration.current + 1;
    importGeneration.current = generation;
    prepareLock.current = true;
    setImportState({ kind: "preparing" });
    setImportNotice(undefined);
    try {
      // 此命令同时打开原生文件选择器并在 Rust 中完整读取/校验 ZIP；前端不接触路径和字节。
      const candidate = await callCommand(commands.protocolPackageImport());
      if (generation !== importGeneration.current) return;
      if (candidate === null) {
        setImportState({ kind: "closed" });
        requestAnimationFrame(() => importTriggerRef.current?.focus());
        return;
      }
      if (!isImportPreview(candidate)) {
        setImportState({ kind: "prepare-error", error: { message: "协议包校验预览数据不完整。", details: [] } });
        return;
      }
      if (candidate.disposition === "identity_conflict") {
        setImportState({ kind: "conflict", preview: withoutImportToken(candidate) });
      } else if (isCommittableImportPreview(candidate)) {
        setImportState({ kind: "ready", preview: candidate });
      } else {
        // IPC 结果即使通过生成类型编译，也可能来自旧后端或损坏适配器；确认入口必须关闭。
        setImportState({ kind: "prepare-error", error: { message: "协议包校验预览的冲突状态与安装凭据不一致。", details: [] } });
      }
    } catch (reason) {
      if (generation === importGeneration.current) {
        setImportState({ kind: "prepare-error", error: presentImportError(reason) });
      }
    } finally {
      prepareLock.current = false;
    }
  }

  async function restoreBuiltInExample() {
    if (restoreLock.current || exportLock.current || prepareLock.current || commitLock.current || discardLock.current) return;
    restoreLock.current = true;
    setRestorePending(true);
    setRestoreError(undefined);
    setImportNotice(undefined);
    try {
      const result = await callCommand(commands.protocolPackageRestoreBuiltin());
      const resultError = builtInRestoreResultError(result);
      if (resultError) {
        setRestoreError(resultError);
        return;
      }
      const refreshed = await callCommand(commands.protocolPackageList());
      if (!isProtocolPackageGroupList(refreshed)) {
        setRestoreError("内置示例已恢复，但刷新后的协议包列表数据不完整。");
        return;
      }
      const packageRef = result.version.package;
      const exactGroup = refreshed.find((item) => item.id === packageRef.id);
      const exactVersion = exactGroup?.versions.find((item) =>
        item.package.version === packageRef.version
        && item.built_in
        && item.enabled
        && item.validation.state === "valid");
      if (!exactGroup || !exactVersion) {
        setRestoreError("内置示例已恢复，但列表中未找到官方精确版本。");
        return;
      }
      packages.setData(refreshed);
      setSelectedKind("socket");
      setSelectedGroup(exactGroup);
      setSelectedVersion(exactVersion);
      setImportNotice(result.outcome === "reused"
        ? "官方 ISO 8583 示例已存在并通过重新校验。"
        : "官方 ISO 8583 示例已恢复并启用。");
      setDialogOpen(true);
    } catch (reason) {
      setRestoreError(presentImportError(reason).message);
    } finally {
      restoreLock.current = false;
      setRestorePending(false);
    }
  }

  async function exportBuiltInTemplate() {
    if (exportLock.current || restoreLock.current || prepareLock.current || commitLock.current || discardLock.current || importState.kind !== "closed") return;
    exportLock.current = true;
    setExportPending(true);
    try {
      const result = await callCommand(commands.protocolPackageExportBuiltin());
      if (result) {
        toast(`ISO 8583 模板 ZIP 已导出（${result.bytes_written} 字节${result.replaced_existing ? "，已覆盖原文件" : ""}）。`, { variant: "success" });
      }
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      exportLock.current = false;
      setExportPending(false);
    }
  }

  async function commitImport() {
    if (importState.kind !== "ready" || prepareLock.current || commitLock.current) return;
    const generation = importGeneration.current;
    const { preview } = importState;
    commitLock.current = true;
    // 从这一行开始 token 已交给消费型命令；React 状态只保留不含 token 的展示副本。
    setImportState({ kind: "committing", preview: withoutImportToken(preview) });
    try {
      const result = await callCommand(commands.protocolPackageImportCommit(preview.token));
      if (generation !== importGeneration.current) return;
      const resultError = importResultError(result, preview);
      if (resultError) {
        setImportState({ kind: "commit-error", error: { message: resultError, details: [] } });
        return;
      }
      setImportState({ kind: "refreshing", packageRef: preview.package, outcome: result.outcome });
      await refreshAfterImport(preview.package, result.outcome, generation);
    } catch (reason) {
      if (generation === importGeneration.current) {
        setImportState({ kind: "commit-error", error: presentImportError(reason) });
      }
    } finally {
      commitLock.current = false;
    }
  }

  async function refreshAfterImport(
    packageRef: { id: string; version: string },
    outcome: "installed" | "reused",
    generation = importGeneration.current,
  ) {
    setImportState({ kind: "refreshing", packageRef, outcome });
    packages.invalidate(false);
    try {
      const refreshed = await callCommand(commands.protocolPackageList());
      if (generation !== importGeneration.current) return;
      if (!isProtocolPackageGroupList(refreshed)) throw new Error("INVALID_REFRESH_RESPONSE");
      const exactGroup = refreshed.find((item) => item.id === packageRef.id);
      const exactVersion = exactGroup?.versions.find((item) => item.package.version === packageRef.version);
      if (!exactGroup || !exactVersion) throw new Error("EXACT_VERSION_MISSING");
      packages.setData(refreshed);
      setSelectedKind(exactGroup.kind);
      setSelectedGroup(exactGroup);
      setSelectedVersion(exactVersion);
      setImportNotice(outcomeText(outcome));
      setImportState({ kind: "closed" });
      setDialogOpen(true);
    } catch (reason) {
      if (generation !== importGeneration.current) return;
      const error = reason instanceof Error && reason.message === "EXACT_VERSION_MISSING"
        ? { message: "列表刷新成功，但未找到刚安装的精确协议包版本。", details: [] }
        : reason instanceof Error && reason.message === "INVALID_REFRESH_RESPONSE"
          ? { message: "刷新后的协议包列表数据不完整。", details: [] }
          : presentImportError(reason);
      setImportState({
        kind: "refresh-error",
        packageRef,
        outcome,
        error,
      });
    }
  }

  function changeImportOpen(open: boolean) {
    if ((prepareLock.current || commitLock.current || discardLock.current) && !open) return;
    if (!open) {
      if (importState.kind === "ready" || importState.kind === "discard-error") {
        void discardAndClose(importState.preview);
        return;
      }
      importGeneration.current += 1;
      setImportState({ kind: "closed" });
      requestAnimationFrame(() => importTriggerRef.current?.focus());
    }
  }

  async function discardAndClose(preview: CommittableImportPreview) {
    if (discardLock.current) return;
    discardLock.current = true;
    const generation = importGeneration.current;
    setImportState({ kind: "discarding", preview: withoutImportToken(preview) });
    try {
      await callCommand(commands.protocolPackageImportDiscard(preview.token));
      if (generation === importGeneration.current) {
        importGeneration.current += 1;
        setImportState({ kind: "closed" });
        requestAnimationFrame(() => importTriggerRef.current?.focus());
      }
    } catch (reason) {
      if (generation === importGeneration.current) {
        const error = presentImportError(reason);
        if (error.code === "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID") {
          // 过期、已消费或已释放都会统一返回 TOKEN_INVALID；这些情况都表示后端已无
          // 对应的待确认资源。继续保留同一 token 只会让用户陷入永久失败的重试循环。
          importGeneration.current += 1;
          setImportState({ kind: "closed" });
          requestAnimationFrame(() => importTriggerRef.current?.focus());
        } else {
          setImportState({ kind: "discard-error", preview, error });
        }
      }
    } finally {
      discardLock.current = false;
    }
  }

  return (
    <ProtocolWorkspaceTabs
      ariaLabel="协议包类型"
      selectedKey={selectedKind}
      onSelectionChange={setSelectedKind}
    >
    <section className="min-w-0 space-y-4 overflow-auto p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 ref={headingRef} tabIndex={-1} className="sr-only">协议包</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            {selectedKind === "http"
              ? "查看用于解析 HTTP 文本 Body 的协议包、精确版本与双向 Schema。"
              : "查看用于解析 Socket 报文的协议包、精确版本与双向 Schema。"}
          </p>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          {selectedKind === "socket" ? (
            <>
              <Button
                variant="outline"
                isDisabled={restorePending || exportPending || importState.kind !== "closed"}
                onPress={() => void restoreBuiltInExample()}
              >
                {restorePending ? "正在恢复…" : "恢复 ISO 8583 示例包"}
              </Button>
              <Button
                variant="outline"
                isDisabled={exportPending || restorePending || importState.kind !== "closed"}
                onPress={() => void exportBuiltInTemplate()}
              >
                {exportPending ? "正在导出…" : "导出 ISO 8583 模板 ZIP"}
              </Button>
            </>
          ) : null}
          <Button
            ref={importTriggerRef}
            variant="primary"
            isDisabled={prepareLock.current || commitLock.current || restorePending || exportPending}
            onPress={() => void chooseZip()}
          >
            导入协议包 ZIP
          </Button>
        </div>
      </div>
      {importNotice && <p role="status" className="text-sm text-success">{importNotice}</p>}
      {restoreError && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>内置示例恢复失败</Alert.Title>
            <Alert.Description>{restoreError}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={() => void restoreBuiltInExample()}>重试</Button>
        </Alert>
      )}
      {selectedKind === "socket" ? <Alert status="accent">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>内置 ISO 8583:1987 ASCII Profile</Alert.Title>
          <Alert.Description>
            模板覆盖主位图、次位图和 DE2–DE128 字段结构；2 字节大端长度头属于当前 Socket 传输约定。接入真实系统前，仍需按对端的字段编码和私有域规格调整。
          </Alert.Description>
        </Alert.Content>
      </Alert> : null}
      {listError && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>协议包列表读取失败</Alert.Title>
            <Alert.Description>{listError}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={() => void packages.refresh()}>重试</Button>
        </Alert>
      )}
      {packages.isLoading ? (
        <div className="grid min-h-56 place-items-center"><Spinner aria-label="正在读取协议包列表" /></div>
      ) : !listError && groups.length === 0 ? (
        <div className="rounded-xl border border-dashed border-[var(--telemetry-line)] p-10 text-center">
          <p className="font-medium">尚未安装 {selectedKind === "http" ? "HTTP" : "Socket"} 协议包</p>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">导入对应类型的 ZIP 后可在此查看版本、能力与 Schema。</p>
          {selectedKind === "socket" ? <div className="mt-4 flex flex-wrap justify-center gap-2">
            <Button variant="primary"
              isDisabled={restorePending || exportPending || importState.kind !== "closed"}
              onPress={() => void restoreBuiltInExample()}>
              {restorePending ? "正在恢复…" : "恢复 ISO 8583 示例包"}
            </Button>
            <Button variant="outline"
              isDisabled={exportPending || restorePending || importState.kind !== "closed"}
              onPress={() => void exportBuiltInTemplate()}>
              {exportPending ? "正在导出…" : "导出 ISO 8583 模板 ZIP"}
            </Button>
          </div> : null}
        </div>
      ) : !listError ? (
        <div className="overflow-hidden rounded-xl border border-[var(--telemetry-line)]">
          <div className="grid grid-cols-[minmax(0,2fr)_minmax(8rem,1fr)_7rem_7rem_7rem] gap-3 bg-[var(--telemetry-table-head)] px-4 py-2 text-xs font-semibold text-[var(--telemetry-muted)] max-[760px]:hidden" aria-hidden="true">
            <span>协议包</span><span>最新版本</span><span>版本数</span><span>引用数</span><span>状态</span>
          </div>
          {groups.map((group) => (
            <ProtocolPackageRow key={group.id} group={group} onOpen={(trigger) => openGroup(group, trigger)} />
          ))}
        </div>
      ) : null}
      <ProtocolPackageDialog
        group={selectedGroup}
        selectedVersion={selectedVersion}
        isOpen={dialogOpen}
        announcement={importNotice}
        onVersionChange={setSelectedVersion}
        onOpenChange={changeOpen}
      />
      <ProtocolPackageImportDialog
        state={importState}
        onOpenChange={changeImportOpen}
        onChoose={() => void chooseZip()}
        onCommit={() => void commitImport()}
        onRefresh={() => {
          if (importState.kind === "refresh-error") {
            void refreshAfterImport(importState.packageRef, importState.outcome);
          }
        }}
      />
    </section>
    </ProtocolWorkspaceTabs>
  );
}
