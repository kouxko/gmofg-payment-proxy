import { Button } from "@heroui/react";
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

interface SessionDetailPanelProps {
  selected?: SessionSummaryViewModel;
  detail: DetailQuery;
  onClose: () => void;
}

export function SessionDetailPanel({
  selected,
  detail,
  onClose,
}: SessionDetailPanelProps) {
  return (
    <aside className="hidden min-w-0 overflow-auto border-l border-[var(--telemetry-line)] p-4 min-[1281px]:block">
      {selected && (
        <Button
          className="mb-3 ml-auto"
          size="sm"
          variant="ghost"
          onPress={onClose}
        >
          关闭详情并释放报文
        </Button>
      )}
      <SessionDetailContent selected={selected} detail={detail} />
    </aside>
  );
}
