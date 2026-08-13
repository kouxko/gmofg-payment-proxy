import { Alert, Button, Chip, Spinner, Tooltip } from "@heroui/react";
import { ArrowRotateRight } from "@gravity-ui/icons";
import type { BreakpointSummaryViewModel } from "@/generated/rust-types";
import { formatTimestamp, toneColor } from "@/lib/format";

interface BreakpointQueuePanelProps {
  data?: BreakpointSummaryViewModel[];
  error?: string;
  isLoading: boolean;
  selectedId?: string;
  onRefresh: () => void;
  onSelect: (id: string) => void;
}

export function BreakpointQueuePanel(props: BreakpointQueuePanelProps) {
  return (
    <aside className="overflow-auto border-r border-[var(--telemetry-line)] p-3 max-[820px]:max-h-64 max-[820px]:border-r-0 max-[820px]:border-b">
      <div className="mb-3 flex items-center">
        <h1 className="text-lg font-semibold">
          暂停队列 ({props.data?.length ?? 0})
        </h1>
        <Tooltip delay={0}>
          <Button
            className="ml-auto"
            isIconOnly
            size="sm"
            variant="ghost"
            aria-label="刷新断点队列"
            onPress={props.onRefresh}
          >
            <ArrowRotateRight className="size-4" />
          </Button>
          <Tooltip.Content>刷新断点队列</Tooltip.Content>
        </Tooltip>
      </div>
      <div className="space-y-3">
        {props.error && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>断点队列读取失败</Alert.Title>
              <Alert.Description>{props.error}</Alert.Description>
            </Alert.Content>
            <Button size="sm" variant="outline" onPress={props.onRefresh}>
              重试
            </Button>
          </Alert>
        )}
        {(props.data ?? []).map((item) => (
          <Button
            key={item.breakpoint_id}
            data-breakpoint-card
            variant={
              item.breakpoint_id === props.selectedId ? "primary" : "outline"
            }
            className="h-auto min-w-0 w-full max-w-full justify-start overflow-hidden px-3 py-3 text-left"
            onPress={() => props.onSelect(item.breakpoint_id)}
          >
            <div
              data-breakpoint-card-content
              className="min-w-0 flex-1 space-y-2 overflow-hidden"
            >
              <div className="grid min-w-0 grid-cols-[auto_auto_minmax(0,1fr)] items-center gap-2">
                <Chip size="sm" color={toneColor(item.ui_tone)} variant="soft">
                  {item.stage === "request" ? "请求断点" : "响应断点"}
                </Chip>
                <span className="whitespace-nowrap">{item.terminal_ip}</span>
                <span
                  data-breakpoint-channel
                  className="min-w-0 truncate text-right"
                  title={item.channel_text}
                >
                  {item.channel_text}
                </span>
              </div>
              <div className="truncate font-mono text-xs">
                {item.method} {item.target}
              </div>
              <div className="flex text-xs">
                <span>{formatTimestamp(item.waiting_since)}</span>
                <span className="ml-auto">
                  {item.certificate_fingerprint_suffix}
                </span>
              </div>
            </div>
          </Button>
        ))}
        {!props.isLoading && !props.error && props.data?.length === 0 && (
          <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
            当前没有待处理断点
          </p>
        )}
        {props.isLoading && <Spinner aria-label="正在加载断点队列" />}
      </div>
    </aside>
  );
}
