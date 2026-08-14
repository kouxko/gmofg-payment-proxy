import { Button, Modal } from "@heroui/react";
import type {
  ProtocolPackageDetailViewModel,
  ProtocolPackageGroupViewModel,
  ProtocolPackageVersionViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import { ProtocolPackageDetail } from "./protocol-package-detail";
import { protocolPackageDetailError } from "./protocol-package-model";
import { ProtocolPackageVersionList } from "./protocol-package-version-list";

export function ProtocolPackageDialog({
  group,
  selectedVersion,
  isOpen,
  onVersionChange,
  onOpenChange,
}: {
  group?: ProtocolPackageGroupViewModel;
  selectedVersion?: ProtocolPackageVersionViewModel;
  isOpen: boolean;
  onVersionChange: (version: ProtocolPackageVersionViewModel) => void;
  onOpenChange: (open: boolean) => void;
}) {
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
            </Modal.Header>
            <Modal.Body className="min-h-0 overflow-y-auto">
              <div className="grid min-w-0 gap-5 lg:grid-cols-[14rem_minmax(0,1fr)]">
                <ProtocolPackageVersionList
                  versions={group ? group.versions : []}
                  selectedVersion={selectedVersion?.package.version}
                  onSelect={onVersionChange}
                />
                <ProtocolPackageDetail detail={visibleDetail} />
              </div>
            </Modal.Body>
            <Modal.Footer className="shrink-0 border-t border-[var(--telemetry-line)] pt-4">
              <Button slot="close" variant="outline">关闭协议包详情</Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
