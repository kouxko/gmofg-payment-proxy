import { AlertDialog, Button, Modal } from "@heroui/react";
import { TrashBin, Xmark } from "@gravity-ui/icons";
import type {
  SessionDetailViewModel,
  SessionSummaryViewModel,
} from "@/generated/rust-types";
import { SessionDetailContent } from "./session-detail-content";

interface DetailQuery {
  data?: SessionDetailViewModel;
  error?: string;
  isLoading: boolean;
  refresh: () => Promise<void>;
}

interface SessionActionsProps {
  selected?: SessionSummaryViewModel;
  detail: DetailQuery;
  detailOpen: boolean;
  clearDialogOpen: boolean;
  clearPending: boolean;
  onDetailOpenChange: (open: boolean) => void;
  onClearDialogOpenChange: (open: boolean) => void;
  onClear: () => void;
}

export function SessionActions(props: SessionActionsProps) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <Modal isOpen={props.detailOpen} onOpenChange={props.onDetailOpenChange}>
        <Button className="hidden" aria-hidden="true">
          打开会话详情
        </Button>
        <Modal.Backdrop isDismissable>
          <Modal.Container size="cover" scroll="inside">
            <Modal.Dialog>
              <Modal.Header className="items-start gap-1 pr-12 text-left">
                <Modal.Heading className="text-left text-lg font-semibold">
                  完整会话报文
                </Modal.Heading>
                <p className="max-w-full truncate text-left text-xs text-[var(--telemetry-muted)]">
                  {props.selected
                    ? `${props.selected.method} ${props.selected.target} · ${props.selected.terminal_ip}`
                    : "请求、响应与原始字节仅保留在当前会话"}
                </p>
                <Modal.CloseTrigger aria-label="关闭会话详情并释放报文">
                  <Xmark className="size-4" />
                </Modal.CloseTrigger>
              </Modal.Header>
              <Modal.Body className="min-h-0">
                <SessionDetailContent
                  selected={props.selected}
                  detail={props.detail}
                />
              </Modal.Body>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal>

      <AlertDialog
        isOpen={props.clearDialogOpen}
        onOpenChange={(open) => {
          if (!open && props.clearPending) return;
          props.onClearDialogOpenChange(open);
        }}
      >
        <Button variant="danger-soft">
          <TrashBin className="size-4" />
          清空全部会话
        </Button>
        <AlertDialog.Backdrop>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header>
                <AlertDialog.Heading>清空已完成会话？</AlertDialog.Heading>
              </AlertDialog.Header>
              <AlertDialog.Body>
                待处理断点不会被清空，此操作不可撤销。
              </AlertDialog.Body>
              <AlertDialog.Footer>
                <Button
                  slot="close"
                  variant="outline"
                  isDisabled={props.clearPending}
                >
                  取消
                </Button>
                <Button
                  variant="danger"
                  isDisabled={props.clearPending}
                  onPress={props.onClear}
                >
                  {props.clearPending ? "正在清空…" : "确认清空"}
                </Button>
              </AlertDialog.Footer>
            </AlertDialog.Dialog>
          </AlertDialog.Container>
        </AlertDialog.Backdrop>
      </AlertDialog>
    </div>
  );
}
