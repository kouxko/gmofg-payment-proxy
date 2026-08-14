"use client";

import { useRef, useState } from "react";
import { Alert, Button, Spinner } from "@heroui/react";
import type {
  ProtocolPackageGroupViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ProtocolPackageDialog } from "./protocol-package-dialog";
import { ProtocolPackageRow } from "./protocol-package-row";
import {
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
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const headingRef = useRef<HTMLHeadingElement | null>(null);
  const listIsValid = isProtocolPackageGroupList(packages.data);
  const listError = packages.error
    ?? (!packages.isLoading && !listIsValid
      ? "协议包列表返回了不完整的数据。"
      : undefined);
  const groups: ProtocolPackageGroupViewModel[] = listIsValid && packages.data
    ? packages.data
    : [];

  function openGroup(group: ProtocolPackageGroupViewModel, trigger: HTMLButtonElement) {
    triggerRef.current = trigger;
    setSelectedGroup(group);
    setSelectedVersion(sortPackageVersions(group.versions ?? [])[0]);
    setDialogOpen(true);
  }

  function changeOpen(open: boolean) {
    setDialogOpen(open);
    if (!open) {
      requestAnimationFrame(() => {
        const focusTarget = triggerRef.current?.isConnected
          ? triggerRef.current
          : headingRef.current;
        focusTarget?.focus();
      });
    }
  }

  return (
    <section className="min-w-0 space-y-4 overflow-auto p-5">
      <div>
        <h1 ref={headingRef} tabIndex={-1} className="text-2xl font-semibold">协议包</h1>
        <p className="mt-1 text-sm text-[var(--telemetry-muted)]">查看已安装 Socket 协议包、精确版本、能力与 Schema。</p>
      </div>
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
          <p className="font-medium">尚未安装协议包</p>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">安装后可在此查看版本、能力与 Schema。</p>
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
        onVersionChange={setSelectedVersion}
        onOpenChange={changeOpen}
      />
    </section>
  );
}
