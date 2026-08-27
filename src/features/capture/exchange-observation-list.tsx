import { Alert, Button, Chip, Spinner, Table } from "@heroui/react";
import { TrashBin } from "@gravity-ui/icons";
import type {
  ExchangeObservationPage,
  ExchangeObservationRecord,
} from "@/generated/rust-types";
import { formatTimestamp } from "@/lib/format";
import { eventCounts, finalOutcome, openedAt } from "./exchange-observation-model";

interface Props {
  page?: ExchangeObservationPage;
  error?: string;
  loading: boolean;
  selectedId?: string;
  onSelect: (record: ExchangeObservationRecord) => void;
  onPage: (page: number) => void;
  onRetry: () => void;
  onClear: () => void;
}

export function ExchangeObservationList(props: Props) {
  const currentPage = props.page?.page ?? 1;
  const totalPages = Math.max(
    1,
    Math.ceil((props.page?.total ?? 0) / (props.page?.page_size ?? 50)),
  );
  return (
    <div id="exchange-observation-list" className="min-w-0 space-y-4 overflow-auto p-5" tabIndex={-1}>
      <header className="flex items-start gap-4">
        <div>
          <h2 className="text-lg font-semibold">运行记录</h2>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            HTTP 与 Socket 的每个 App 连接都按实际发生顺序保留收到、发送、失败与关闭事件
          </p>
        </div>
        <Button id="exchange-observation-clear" className="ml-auto" variant="danger-soft" isDisabled={props.loading} onPress={props.onClear}>
          <TrashBin className="size-4" />清空运行记录
        </Button>
      </header>
      {Boolean(props.page?.evicted_records || props.page?.dropped_events || props.page?.ignored_events) && (
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>观测数据发生淘汰</Alert.Title>
            <Alert.Description>
              当前工作区已淘汰连接 {props.page?.evicted_records ?? 0} 条；producer 队列丢弃 {props.page?.dropped_events ?? 0} 条；consumer/store 忽略 {props.page?.ignored_events ?? 0} 条。交易数据面未受影响。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      )}
      {props.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>运行记录读取失败</Alert.Title><Alert.Description>{props.error}</Alert.Description></Alert.Content><Button size="sm" variant="outline" onPress={props.onRetry}>重试</Button></Alert>}
      <Table>
        <Table.ScrollContainer>
          <Table.Content
            aria-label="HTTP 与 Socket 运行记录"
            className="min-w-[960px]"
            selectionMode={props.loading ? "none" : "single"}
            selectedKeys={props.selectedId ? [props.selectedId] : []}
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              const id = String(Array.from(keys)[0] ?? "");
              const record = props.page?.rows.find((row) => row.exchange_id === id);
              if (record) props.onSelect(record);
            }}
          >
            <Table.Header>
              <Table.Column isRowHeader>建立时间</Table.Column>
              <Table.Column>协议</Table.Column>
              <Table.Column>对端</Table.Column>
              <Table.Column>收到 / 发送 / 失败</Table.Column>
              <Table.Column>结果</Table.Column>
              <Table.Column>Exchange ID</Table.Column>
            </Table.Header>
            <Table.Body renderEmptyState={() => <div className="p-10 text-center text-sm text-[var(--telemetry-muted)]">{props.loading ? "正在查询运行记录…" : "当前工作区还没有运行记录"}</div>}>
              {(props.page?.rows ?? []).map((record) => {
                const counts = eventCounts(record);
                const outcome = finalOutcome(record);
                return <Table.Row key={record.exchange_id} id={record.exchange_id}>
                  <Table.Cell><Button id={`exchange-observation-row-${record.exchange_id}`} size="sm" variant="ghost" onPress={() => props.onSelect(record)}>{openedAt(record) ? formatTimestamp(openedAt(record)!) : "未知"}</Button></Table.Cell>
                  <Table.Cell><Chip size="sm" color="accent" variant="soft">{record.protocol.toUpperCase()}</Chip></Table.Cell>
                  <Table.Cell><code className="text-xs">{record.peer_address}</code></Table.Cell>
                  <Table.Cell>{counts.received} / {counts.sent} / {counts.failed}</Table.Cell>
                  <Table.Cell><Chip size="sm" color={outcome === "失败" ? "danger" : outcome === "连接中" ? "warning" : "success"} variant="soft">{outcome}</Chip></Table.Cell>
                  <Table.Cell><code className="text-xs">{record.exchange_id}</code></Table.Cell>
                </Table.Row>;
              })}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
        <Table.Footer className="flex items-center justify-between px-4 py-3 text-sm">
          <span>当前 {props.page?.rows.length ?? 0} 条，共 {props.page?.total ?? 0} 条</span>
          <div className="flex items-center gap-2"><Button size="sm" variant="outline" isDisabled={props.loading || currentPage <= 1} onPress={() => props.onPage(currentPage - 1)}>上一页</Button><span>{Math.min(currentPage, totalPages)} / {totalPages}</span><Button size="sm" variant="outline" isDisabled={props.loading || currentPage >= totalPages} onPress={() => props.onPage(currentPage + 1)}>下一页</Button></div>
          {props.loading && <Spinner size="sm" aria-label="正在刷新运行记录" />}
        </Table.Footer>
      </Table>
    </div>
  );
}
