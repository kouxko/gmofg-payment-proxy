import { useRef } from "react";
import { Button, Chip } from "@heroui/react";
import type { ProtocolPackageGroupViewModel } from "@/generated/rust-types";
import {
  isBuiltInPackage,
  isExternalPackage,
  isManagedPackage,
  packageStatus,
  sortPackageVersions,
} from "./protocol-package-model";

export function ProtocolPackageRow({
  group,
  onOpen,
}: {
  group: ProtocolPackageGroupViewModel;
  onOpen: (trigger: HTMLButtonElement) => void;
}) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const versions = sortPackageVersions(group.versions);
  const status = packageStatus(versions);
  return (
    <Button
      ref={triggerRef}
      variant="ghost"
      className="grid h-auto min-h-16 w-full cursor-pointer grid-cols-[minmax(0,2fr)_minmax(8rem,1fr)_7rem_7rem_7rem] items-center gap-3 rounded-none border-b border-[var(--telemetry-line)] px-4 py-3 text-left outline-none last:border-b-0 hover:bg-[var(--telemetry-table-head)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--telemetry-accent)] max-[760px]:grid-cols-2"
      aria-label={`查看协议包 ${group.name}`}
      onPress={() => {
        if (triggerRef.current) onOpen(triggerRef.current);
      }}
    >
      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium">{group.name}</span>
          <Chip size="sm" variant="soft">
            {group.kind === "http" ? "HTTP" : "Socket"}
          </Chip>
        </span>
        {versions.some(isBuiltInPackage) && (
          <Chip size="sm" color="accent" variant="soft">内置示例</Chip>
        )}
        {versions.some(isExternalPackage) && (
          <Chip size="sm" color="warning" variant="soft">远端调试</Chip>
        )}
        {versions.some(isManagedPackage) && (
          <Chip size="sm" color="accent" variant="soft">本地管理</Chip>
        )}
        <span className="block truncate font-mono text-xs text-[var(--telemetry-muted)]">
          {group.id}
        </span>
      </span>
      <span className="truncate font-mono text-sm">{versions[0].package.version}</span>
      <span>{versions.length} 个版本</span>
      <span>{group.reference_count} 个引用</span>
      <span className="flex flex-wrap gap-1">
        <Chip size="sm" color={status.color} variant="soft">{status.label}</Chip>
        {versions.some((version) => version.package_source.type === "external" && version.package_source.online) && (
          <Chip size="sm" color="success" variant="soft">远端在线</Chip>
        )}
        {versions.some((version) => version.package_source.type === "external" && !version.package_source.online) && (
          <Chip size="sm" color="danger" variant="soft">远端离线</Chip>
        )}
        {versions.some((version) => version.package_source.type === "managed" && version.package_source.online) && (
          <Chip size="sm" color="success" variant="soft">本地运行中</Chip>
        )}
        {versions.some((version) => version.package_source.type === "managed" && !version.package_source.online) && (
          <Chip size="sm" color="danger" variant="soft">本地已停止</Chip>
        )}
        {status.invalidCount > 0 && status.validCount > 0 && (
          <Chip size="sm" color="danger" variant="soft">
            {status.invalidCount} 个校验异常
          </Chip>
        )}
        {group.active_reference_count > 0 && (
          <Chip size="sm" color="accent" variant="soft">
            {group.active_reference_count} 个运行中
          </Chip>
        )}
      </span>
    </Button>
  );
}
