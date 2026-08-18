"use client";

import { useState } from "react";
import { Xmark } from "@gravity-ui/icons";
import { Button, Modal } from "@heroui/react";
import type { ProtocolPackageDetailViewModel, ProtocolPackageRef } from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { ProtocolPackageDetail } from "@/features/protocol-packages/protocol-package-detail";
import { protocolPackageDetailError } from "@/features/protocol-packages/protocol-package-model";
import { callCommand } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";

export function SocketProtocolPackageDialog({ packageRef, disabled }: {
  packageRef: ProtocolPackageRef;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const triggerId = `socket-package-detail-${packageRef.id}-${packageRef.version}`;
  const detail = useIpcQuery<ProtocolPackageDetailViewModel>(
    `listener-package-detail:${packageRef.id}@${packageRef.version}`,
    () => callCommand(commands.protocolPackageDetail(packageRef)),
    undefined,
    { enabled: open },
  );
  const responseError = detail.data
    ? protocolPackageDetailError(detail.data, packageRef)
    : undefined;
  return (
    <>
      <Button id={triggerId} variant="outline" isDisabled={disabled} onPress={() => setOpen(true)}>
        查看所选版本与 Schema
      </Button>
      <Modal isOpen={open} onOpenChange={(next) => {
        setOpen(next);
        if (!next) requestAnimationFrame(() => document.getElementById(triggerId)?.focus());
      }}>
        <Button className="hidden" aria-hidden="true">打开入口协议包详情</Button>
        <Modal.Backdrop isDismissable>
          <Modal.Container size="cover" scroll="inside">
            <Modal.Dialog>
              <Modal.Header className="items-start gap-1 pr-12 text-left">
                <Modal.Heading className="text-left text-lg font-semibold">入口协议包详情</Modal.Heading>
                <p className="break-all text-left font-mono text-xs text-[var(--telemetry-muted)]">
                  {packageRef.id}@{packageRef.version}
                </p>
                <Modal.CloseTrigger aria-label="关闭协议包详情">
                  <Xmark className="size-4" />
                </Modal.CloseTrigger>
              </Modal.Header>
              <Modal.Body className="min-h-0 overflow-y-auto">
                <ProtocolPackageDetail detail={responseError
                  ? { data: undefined, error: responseError, isLoading: false }
                  : detail} />
              </Modal.Body>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal>
    </>
  );
}
