import { useRef } from "react";
import { Button, Chip } from "@heroui/react";
import type { ProtocolPackageGroupViewModel } from "@/generated/rust-types";
import { sortPackageVersions } from "./protocol-package-model";

export function ProtocolPackageRow({
  group,
  onOpen,
}: {
  group: ProtocolPackageGroupViewModel;
  onOpen: (trigger: HTMLButtonElement) => void;
}) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const versions = sortPackageVersions(group.versions);
  const online = versions.some((version) => version.package_source.online);
  return (
    <Button
      ref={triggerRef}
      variant="ghost"
      className="grid h-auto min-h-16 w-full cursor-pointer grid-cols-[minmax(0,2fr)_minmax(8rem,1fr)_7rem_7rem] items-center gap-3 rounded-none border-b border-[var(--telemetry-line)] px-4 py-3 text-left outline-none last:border-b-0 hover:bg-[var(--telemetry-table-head)] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--telemetry-accent)] max-[760px]:grid-cols-2"
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
        <span className="block truncate font-mono text-xs text-[var(--telemetry-muted)]">
          {group.id}
        </span>
      </span>
      <span className="truncate font-mono text-sm">{versions[0].package.version}</span>
      <span>{versions.length} 个版本</span>
      <span>
        <Chip size="sm" color={online ? "success" : "danger"} variant="soft">
          {online ? "在线" : "离线"}
        </Chip>
      </span>
    </Button>
  );
}
