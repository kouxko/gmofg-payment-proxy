import { AlertDialog, Button, Drawer } from "@heroui/react";
import { ArrowDownToLine, Eye, TrashBin } from "@gravity-ui/icons";
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
  exportDialogOpen: boolean;
  exportPending: boolean;
  clearDialogOpen: boolean;
  clearPending: boolean;
  onDetailOpenChange: (open: boolean) => void;
  onExportDialogOpenChange: (open: boolean) => void;
  onExport: () => void;
  onClearDialogOpenChange: (open: boolean) => void;
  onClear: () => void;
}

export function SessionActions(props: SessionActionsProps) {
  return (
    <div className="flex items-center gap-3">
      <Drawer isOpen={props.detailOpen} onOpenChange={props.onDetailOpenChange}>
        <Button isDisabled={!props.selected} variant="outline">
          <Eye className="size-4" />
          查看完整报文
        </Button>
        <Drawer.Backdrop>
          <Drawer.Content placement="right">
            <Drawer.Dialog>
              <Drawer.Header>
                <Drawer.Heading>完整会话报文</Drawer.Heading>
              </Drawer.Header>
              <Drawer.Body>
                <SessionDetailContent
                  selected={props.selected}
                  detail={props.detail}
                />
              </Drawer.Body>
              <Drawer.Footer>
                <Button slot="close" variant="outline">
                  关闭
                </Button>
              </Drawer.Footer>
            </Drawer.Dialog>
          </Drawer.Content>
        </Drawer.Backdrop>
      </Drawer>

      <AlertDialog
        isOpen={props.exportDialogOpen}
        onOpenChange={(open) => {
          if (!open && props.exportPending) return;
          props.onExportDialogOpenChange(open);
        }}
      >
        <Button variant="outline" isDisabled={!props.selected}>
          <ArrowDownToLine className="size-4" />
          导出所选会话
        </Button>
        <AlertDialog.Backdrop>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header>
                <AlertDialog.Heading>确认导出原始报文</AlertDialog.Heading>
              </AlertDialog.Header>
              <AlertDialog.Body>
                导出的 JSON 文件包含原始敏感数据。保存位置和文件写入均由 Rust
                原生侧处理。
              </AlertDialog.Body>
              <AlertDialog.Footer>
                <Button
                  slot="close"
                  variant="outline"
                  isDisabled={props.exportPending}
                >
                  取消
                </Button>
                <Button
                  variant="primary"
                  isDisabled={props.exportPending}
                  onPress={props.onExport}
                >
                  {props.exportPending ? "正在导出…" : "确认并选择位置"}
                </Button>
              </AlertDialog.Footer>
            </AlertDialog.Dialog>
          </AlertDialog.Container>
        </AlertDialog.Backdrop>
      </AlertDialog>

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
