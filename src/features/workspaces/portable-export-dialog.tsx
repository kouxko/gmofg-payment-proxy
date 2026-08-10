"use client";

import { AlertDialog, Button } from "@heroui/react";
import { ArrowDownToLine } from "@gravity-ui/icons";

interface PortableExportDialogProps {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  triggerLabel: string;
  heading: string;
  description: string;
  confirmLabel: string;
  isDisabled?: boolean;
  fullWidth?: boolean;
  onConfirm: () => void;
}

/**
 * 可移植配置可能携带测试证书和明文 P12 密码，因此每个导出入口都必须先明确确认。
 * 对话框仅收集用户意图；文件组装、敏感材料筛选与保存仍全部由 Rust 完成。
 */
export function PortableExportDialog({
  isOpen,
  onOpenChange,
  triggerLabel,
  heading,
  description,
  confirmLabel,
  isDisabled,
  fullWidth,
  onConfirm,
}: PortableExportDialogProps) {
  return (
    <AlertDialog isOpen={isOpen} onOpenChange={onOpenChange}>
      <Button fullWidth={fullWidth} variant="outline" isDisabled={isDisabled}>
        <ArrowDownToLine className="size-4" />
        {triggerLabel}
      </Button>
      <AlertDialog.Backdrop>
        <AlertDialog.Container>
          <AlertDialog.Dialog>
            <AlertDialog.Header>
              <AlertDialog.Heading>{heading}</AlertDialog.Heading>
            </AlertDialog.Header>
            <AlertDialog.Body>{description}</AlertDialog.Body>
            <AlertDialog.Footer>
              <Button slot="close" variant="outline">取消</Button>
              <Button variant="danger" onPress={onConfirm}>{confirmLabel}</Button>
            </AlertDialog.Footer>
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog>
  );
}
