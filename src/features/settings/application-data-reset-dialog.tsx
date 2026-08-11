"use client";

import { useState } from "react";
import { AlertDialog, Button, toast } from "@heroui/react";
import { TrashBin } from "@gravity-ui/icons";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";

export function ApplicationDataResetDialog({
  isDisabled,
}: {
  isDisabled: boolean;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [isPending, setIsPending] = useState(false);

  async function clearApplicationData() {
    if (isDisabled || isPending) return;
    setIsPending(true);
    try {
      await callCommand(commands.applicationDataReset(true));
      // 原生命令会立即请求应用重启；此分支只用于测试环境或重启被平台延迟时。
      toast("全部配置与测试数据已清除，应用正在重启。", {
        variant: "success",
      });
      setIsOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setIsPending(false);
    }
  }

  return (
    <AlertDialog
      isOpen={isOpen}
      onOpenChange={(open) => {
        if (!open && isPending) return;
        setIsOpen(open);
      }}
    >
      <Button className="ml-3" variant="danger-soft" isDisabled={isDisabled}>
        <TrashBin className="size-4" />
        清除全部配置与数据
      </Button>
      <AlertDialog.Backdrop>
        <AlertDialog.Container>
          <AlertDialog.Dialog>
            <AlertDialog.Header>
              <AlertDialog.Heading>清除全部配置与测试数据？</AlertDialog.Heading>
            </AlertDialog.Header>
            <AlertDialog.Body className="space-y-3">
              <p>
                将停止所有入口和设备网络接管，并删除全部工作区、监听、规则、设备方案、
                会话、抓包、导入的 Listener TLS / PKCS12 / CA 材料及全局设置。
              </p>
              <p>
                清除后应用会自动重启并建立干净的默认工作区。本机外观主题会保留；
                未提前导出的配置无法恢复。
              </p>
            </AlertDialog.Body>
            <AlertDialog.Footer>
              <Button slot="close" variant="outline" isDisabled={isPending}>
                取消
              </Button>
              <Button
                variant="danger"
                isDisabled={isPending}
                onPress={() => void clearApplicationData()}
              >
                {isPending ? "正在清除并重启…" : "确认清除并重启"}
              </Button>
            </AlertDialog.Footer>
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog>
  );
}
