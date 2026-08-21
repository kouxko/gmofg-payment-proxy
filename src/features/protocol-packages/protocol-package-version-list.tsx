import { Button, Chip } from "@heroui/react";
import type { ProtocolPackageVersionViewModel } from "@/generated/rust-types";
import { sortPackageVersions, validationText } from "./protocol-package-model";

export function ProtocolPackageVersionList({
  versions,
  selectedVersion,
  isDisabled = false,
  onSelect,
}: {
  versions: ProtocolPackageVersionViewModel[];
  selectedVersion?: string;
  isDisabled?: boolean;
  onSelect: (version: ProtocolPackageVersionViewModel) => void;
}) {
  const sorted = sortPackageVersions(versions);
  return (
    <nav aria-label="协议包版本" className="space-y-2">
      <h3 className="text-sm font-semibold">已安装版本</h3>
      {sorted.length === 0 ? (
        <p className="text-sm text-[var(--telemetry-muted)]">没有可查看的版本。</p>
      ) : (
        <div className="flex gap-2 overflow-x-auto pb-1 lg:flex-col">
          {sorted.map((version) => {
            const selected = version.package.version === selectedVersion;
            return (
              <Button
                key={version.package.version}
                variant={selected ? "primary" : "outline"}
                className="h-auto min-w-40 justify-between py-2 lg:w-full"
                aria-pressed={selected}
                isDisabled={isDisabled}
                onPress={() => onSelect(version)}
              >
                <span className="font-mono">{version.package.version}</span>
                <Chip
                  size="sm"
                  color={version.validation.state === "valid" ? (version.enabled ? "success" : "default") : "danger"}
                  variant="soft"
                  aria-label={validationText(version.validation)}
                >
                  {version.validation.state === "valid" ? (version.enabled ? "启用" : "停用") : "无效"}
                </Chip>
              </Button>
            );
          })}
        </div>
      )}
    </nav>
  );
}
