import { Alert, Button, Chip, Spinner, Table } from "@heroui/react";
import { TrashBin } from "@gravity-ui/icons";
import type {
  SocketCapturePageViewModel,
  SocketCaptureRowViewModel,
} from "@/generated/rust-types";
import { formatBytes, formatTimestamp } from "@/lib/format";
import { packageLabel, schemaLabel } from "./socket-capture-model";

interface SocketCaptureListProps {
  page?: SocketCapturePageViewModel;
  error?: string;
  loading: boolean;
  selectedId?: string;
  onSelect: (row: SocketCaptureRowViewModel) => void;
  onPage: (page: number) => void;
  onRetry: () => void;
  onClear: () => void;
  clearButtonId: string;
}

const kindLabel = { relay_frame: "转发报文", local_exchange: "本机应答" } as const;
const directionLabel = { upstream: "App → Server", downstream: "Server → App" } as const;
const failureLabel = {
  response_rule: "响应规则失败",
  response_encode: "响应生成失败",
  response_write: "响应写回失败",
} as const;

export function SocketCaptureList(props: SocketCaptureListProps) {
  const totalPages = Math.max(1, props.page?.total_pages ?? 0);
  return (
    <div id="socket-capture-list" className="min-w-0 space-y-4 overflow-auto p-5" tabIndex={-1}>
      <header className="flex items-start gap-4">
        <div>
          <h2 className="text-lg font-semibold">Socket 抓包</h2>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            显示已完成的转发报文、本机应答以及保留了解析证据的失败记录
          </p>
        </div>
        <Button id={props.clearButtonId} className="ml-auto" variant="danger-soft" isDisabled={props.loading} onPress={props.onClear}>
          <TrashBin className="size-4" />清空 Socket 抓包
        </Button>
      </header>
      {props.error && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>Socket 抓包读取失败</Alert.Title>
            <Alert.Description>{props.error}</Alert.Description>
          </Alert.Content>
          <Button size="sm" variant="outline" onPress={props.onRetry}>重试</Button>
        </Alert>
      )}
      <Table>
        <Table.ScrollContainer>
          <Table.Content
            aria-label="Socket 抓包记录"
            className="min-w-[1320px]"
            selectionMode={props.loading ? "none" : "single"}
            selectedKeys={props.selectedId ? [props.selectedId] : []}
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              const id = Array.from(keys)[0];
              const row = props.page?.rows.find((item) => item.capture_id === id);
              if (row) props.onSelect(row);
            }}
          >
            <Table.Header>
              <Table.Column isRowHeader>时间</Table.Column>
              <Table.Column>类型</Table.Column>
              <Table.Column>方向</Table.Column>
              <Table.Column>协议包</Table.Column>
              <Table.Column>字段结构</Table.Column>
              <Table.Column>原始 / 写出 / 解析</Table.Column>
              <Table.Column>结果</Table.Column>
              <Table.Column>规则</Table.Column>
              <Table.Column>连接</Table.Column>
            </Table.Header>
            <Table.Body renderEmptyState={() => (
              <div className="p-10 text-center text-sm text-[var(--telemetry-muted)]">
                {props.loading ? "正在查询 Socket 抓包…" : (props.page?.empty_message ?? "当前工作区还没有 Socket 抓包")}
              </div>
            )}>
              {(props.page?.rows ?? []).map((row) => (
                <Table.Row key={row.capture_id} id={row.capture_id}>
                  <Table.Cell className="whitespace-nowrap">
                    <Button id={`socket-capture-row-${row.capture_id}`} size="sm" variant="ghost" onPress={() => props.onSelect(row)}>
                      {formatTimestamp(row.occurred_at)}
                    </Button>
                  </Table.Cell>
                  <Table.Cell><Chip size="sm" color="accent" variant="soft">{kindLabel[row.kind]}</Chip></Table.Cell>
                  <Table.Cell>{row.direction ? directionLabel[row.direction] : "应用请求 ⇄ 本机应答"}</Table.Cell>
                  <Table.Cell><code className="text-xs">{packageLabel(row.package)}</code></Table.Cell>
                  <Table.Cell><code className="text-xs">{schemaLabel(row.schema)}</code></Table.Cell>
                  <Table.Cell className="whitespace-nowrap text-xs">
                    {formatBytes(row.origin_size_bytes)} / {formatBytes(row.written_size_bytes)} / {formatBytes(row.logical_size_bytes)}
                  </Table.Cell>
                  <Table.Cell>
                    <Chip size="sm" color={row.failure ? "danger" : "success"} variant="soft">
                      {row.failure ? failureLabel[row.failure.stage] : "已写出"}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell>{row.matched_rule_ids.length}</Table.Cell>
                  <Table.Cell><code className="text-xs">{row.connection_id}</code></Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
        <Table.Footer className="flex items-center justify-between px-4 py-3 text-sm">
          <span>当前 {props.page?.rows.length ?? 0} 条，共 {props.page?.total ?? 0} 条</span>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="outline" isDisabled={props.loading || (props.page?.page ?? 1) <= 1} onPress={() => props.onPage((props.page?.page ?? 1) - 1)}>上一页</Button>
            <span>{Math.min(props.page?.page ?? 1, totalPages)} / {totalPages}</span>
            <Button size="sm" variant="outline" isDisabled={props.loading || (props.page?.page ?? 1) >= totalPages} onPress={() => props.onPage((props.page?.page ?? 1) + 1)}>下一页</Button>
          </div>
          {props.loading && <Spinner size="sm" aria-label="正在刷新 Socket 抓包" />}
        </Table.Footer>
      </Table>
    </div>
  );
}
