import { useEffect, useRef, useState } from "react";
import { Button, Modal } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import type {
  ProtocolPackageDetailViewModel,
  ProtocolPackageGroupViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ProtocolPackageDetail } from "./protocol-package-detail";
import { isProtocolPackageVersion, protocolPackageDetailError } from "./protocol-package-model";
import { ProtocolPackageVersionList } from "./protocol-package-version-list";

export function ProtocolPackageDialog({
  group,
  selectedVersion,
  isOpen,
  announcement,
  onVersionChange,
  onVersionEnabled,
  onOpenChange,
}: {
  group?: ProtocolPackageGroupViewModel;
  selectedVersion?: ProtocolPackageVersionViewModel;
  isOpen: boolean;
  announcement?: string;
  onVersionChange: (version: ProtocolPackageVersionViewModel) => void;
  onVersionEnabled: (version: ProtocolPackageVersionViewModel) => void;
  onOpenChange: (open: boolean) => void;
}) {
  const enableLock = useRef(false);
  const [enablePending, setEnablePending] = useState(false);
  const [enableError, setEnableError] = useState<string>();
  const packageRef = selectedVersion?.package;
  const detail = useIpcQuery<ProtocolPackageDetailViewModel>(
    `protocol-package-detail:${packageRef?.id ?? ""}@${packageRef?.version ?? ""}`,
    () => callCommand(commands.protocolPackageDetail({ id: packageRef!.id, version: packageRef!.version })),
    undefined,
    { enabled: isOpen && packageRef != null },
  );
  const responseError = detail.data !== undefined && packageRef
    ? protocolPackageDetailError(detail.data, packageRef)
    : undefined;
  const visibleDetail = responseError
    ? { data: undefined, error: responseError, isLoading: false }
    : detail;

  useEffect(() => {
    setEnableError(undefined);
  }, [isOpen, packageRef?.id, packageRef?.version]);

  async function enableVersion() {
    if (!selectedVersion || selectedVersion.enabled || enableLock.current) return;
    enableLock.current = true;
    setEnablePending(true);
    setEnableError(undefined);
    try {
      const enabled = await callCommand(commands.protocolPackageEnable(selectedVersion.package));
      if (!isProtocolPackageVersion(enabled)
        || enabled.package.id !== selectedVersion.package.id
        || enabled.package.version !== selectedVersion.package.version
        || enabled.enabled !== true
        || enabled.validation.state !== "valid") {
        setEnableError("协议包启用结果不完整，请刷新列表后重试。");
        return;
      }
      if (detail.data) detail.setData({ ...detail.data, version: enabled });
      onVersionEnabled(enabled);
    } catch (reason) {
      setEnableError(errorMessage(reason));
    } finally {
      enableLock.current = false;
      setEnablePending(false);
    }
  }

  return (
    <Modal isOpen={isOpen} onOpenChange={onOpenChange}>
      <Button className="hidden" aria-hidden="true">打开协议包详情</Button>
      <Modal.Backdrop isDismissable>
        <Modal.Container size="cover" scroll="inside">
          <Modal.Dialog>
            <Modal.Header className="items-start gap-1 pr-12 text-left">
              <Modal.Heading className="max-w-full break-words text-left text-lg font-semibold">
                {group?.name || group?.id || "协议包详情"}
              </Modal.Heading>
              <p className="max-w-full break-all text-left font-mono text-xs text-[var(--telemetry-muted)]">{group?.id ?? ""}</p>
              <Modal.CloseTrigger aria-label="关闭协议包详情">
                <Xmark className="size-4" />
              </Modal.CloseTrigger>
            </Modal.Header>
            <Modal.Body className="min-h-0 overflow-y-auto">
              {announcement && <p role="status" className="mb-4 text-sm text-success">{announcement}</p>}
              <div className="grid min-w-0 gap-5 lg:grid-cols-[14rem_minmax(0,1fr)]">
                <ProtocolPackageVersionList
                  versions={group ? group.versions : []}
                  selectedVersion={selectedVersion?.package.version}
                  onSelect={(version) => {
                    setEnableError(undefined);
                    onVersionChange(version);
                  }}
                />
                <ProtocolPackageDetail
                  detail={visibleDetail}
                  enablePending={enablePending}
                  enableError={enableError}
                  onEnable={() => void enableVersion()}
                />
              </div>
            </Modal.Body>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
