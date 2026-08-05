import type { Dispatch, SetStateAction } from "react";
import {
  Alert,
  Button,
  Card,
  Chip,
  Input,
  Label,
  ListBox,
  SearchField,
  Select,
  Spinner,
  Table,
  TextField,
} from "@heroui/react";
import { Pause, Play, TrashBin } from "@gravity-ui/icons";
import type {
  CapturePageViewModel,
  CaptureQuery,
  ChannelPresentationViewModel,
} from "@/generated/rust-types";
import { formatBytes, formatDuration, formatTimestamp } from "@/lib/format";

interface CaptureListPanelProps {
  paused: boolean;
  clearPending: boolean;
  query: CaptureQuery;
  setQuery: Dispatch<SetStateAction<CaptureQuery>>;
  page: {
    data?: CapturePageViewModel;
    error?: string;
    isLoading: boolean;
    refresh: () => Promise<void>;
  };
  channels: ChannelPresentationViewModel[];
  selectedEventId?: number;
  onTogglePaused: () => void;
  onClear: () => void;
  onSelectEvent: (eventId: number) => void;
}

export function CaptureListPanel(props: CaptureListPanelProps) {
  const update = (change: Partial<CaptureQuery>) =>
    props.setQuery((query) => ({
      ...query,
      ...change,
      after_event_id: null,
      page: { ...query.page, page: 1 },
    }));
  return (
    <div className="min-w-0 space-y-4 overflow-auto p-5">
      <header className="flex items-start">
        <div>
          <h1 className="text-2xl font-semibold">实时抓包</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            暂停列表滚动不会影响网络转发、规则或会话记录
          </p>
        </div>
        <div className="ml-auto flex gap-2">
          <Button variant="outline" onPress={props.onTogglePaused}>
            {props.paused ? (
              <Play className="size-4" />
            ) : (
              <Pause className="size-4" />
            )}
            {props.paused ? "恢复列表滚动" : "暂停列表滚动"}
          </Button>
          <Button
            variant="danger-soft"
            isDisabled={props.clearPending}
            onPress={props.onClear}
          >
            <TrashBin className="size-4" />
            {props.clearPending ? "正在清空…" : "清空当前显示"}
          </Button>
        </div>
      </header>
      <Card>
        <Card.Content className="grid grid-cols-[minmax(210px,2fr)_minmax(140px,1fr)_minmax(150px,1fr)_minmax(150px,1fr)] gap-3 p-4 max-[900px]:grid-cols-2">
          <div className="grid min-w-0 gap-1">
            <Label>关键字或请求 ID</Label>
            <SearchField
              aria-label="关键字或请求 ID"
              value={props.query.keyword ?? ""}
              onChange={(keyword) => update({ keyword: keyword || null })}
            >
              <SearchField.Group>
                <SearchField.SearchIcon />
                <SearchField.Input placeholder="关键字 / 请求 ID" />
                <SearchField.ClearButton />
              </SearchField.Group>
            </SearchField>
          </div>
          <TextField>
            <Label>终端 IP</Label>
            <Input
              placeholder="例如 192.168.1.20"
              value={props.query.terminal_ip ?? ""}
              onChange={(event) =>
                update({ terminal_ip: event.target.value || null })
              }
            />
          </TextField>
          <div className="grid min-w-0 gap-1">
            <Label>通道</Label>
            <Select
              aria-label="通道筛选"
              selectedKey={props.query.channel ?? "all"}
              onSelectionChange={(key) =>
                update({
                  channel:
                    key === "all" ? null : (key as CaptureQuery["channel"]),
                })
              }
            >
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>
                  <ListBox.Item id="all" textValue="全部通道">
                    全部通道
                  </ListBox.Item>
                  {props.channels.map((channel) => (
                    <ListBox.Item
                      key={channel.id}
                      id={channel.id}
                      textValue={channel.display_name}
                    >
                      {channel.display_name}
                    </ListBox.Item>
                  ))}
                </ListBox>
              </Select.Popover>
            </Select>
          </div>
          <div className="grid min-w-0 gap-1">
            <Label>阶段</Label>
            <Select
              aria-label="阶段筛选"
              selectedKey={props.query.stage ?? "all"}
              onSelectionChange={(key) =>
                update({
                  stage: key === "all" ? null : (key as CaptureQuery["stage"]),
                })
              }
            >
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>
                  <ListBox.Item id="all">全部阶段</ListBox.Item>
                  <ListBox.Item id="request">请求</ListBox.Item>
                  <ListBox.Item id="response">响应</ListBox.Item>
                  <ListBox.Item id="terminal">终态</ListBox.Item>
                </ListBox>
              </Select.Popover>
            </Select>
          </div>
          <TextField>
            <Label>结果</Label>
            <Input
              placeholder="例如 success / timeout"
              value={props.query.result ?? ""}
              onChange={(event) =>
                update({ result: event.target.value || null })
              }
            />
          </TextField>
          <TextField>
            <Label>规则 ID</Label>
            <Input
              placeholder="命中的规则 ID"
              value={props.query.rule_id ?? ""}
              onChange={(event) =>
                update({ rule_id: event.target.value || null })
              }
            />
          </TextField>
        </Card.Content>
      </Card>
      {props.page.error && (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>抓包列表读取失败</Alert.Title>
            <Alert.Description>{props.page.error}</Alert.Description>
          </Alert.Content>
          <Button
            size="sm"
            variant="outline"
            onPress={() => void props.page.refresh()}
          >
            重试
          </Button>
        </Alert>
      )}
      <Table>
        <Table.ScrollContainer>
          <Table.Content
            aria-label="实时抓包事件"
            className="min-w-[1160px]"
            selectionMode="single"
            selectedKeys={
              props.selectedEventId != null
                ? [String(props.selectedEventId)]
                : []
            }
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              const first = Array.from(keys)[0];
              if (first != null) props.onSelectEvent(Number(first));
            }}
          >
            <Table.Header>
              <Table.Column isRowHeader>时间（毫秒）</Table.Column>
              <Table.Column>终端 IP</Table.Column>
              <Table.Column>通道</Table.Column>
              <Table.Column>方向</Table.Column>
              <Table.Column>方法</Table.Column>
              <Table.Column>路径 / 请求类型</Table.Column>
              <Table.Column>HTTP 状态码</Table.Column>
              <Table.Column>结果</Table.Column>
              <Table.Column>耗时</Table.Column>
              <Table.Column>匹配规则</Table.Column>
              <Table.Column>大小</Table.Column>
            </Table.Header>
            <Table.Body
              renderEmptyState={() => (
                <div className="p-8 text-center">
                  {props.page.isLoading
                    ? "正在查询…"
                    : props.page.data?.empty_message}
                </div>
              )}
            >
              {(props.page.data?.rows ?? []).map((row) => (
                <Table.Row key={row.event_id} id={String(row.event_id)}>
                  <Table.Cell className="whitespace-nowrap">
                    {formatTimestamp(row.occurred_at)}
                  </Table.Cell>
                  <Table.Cell>{row.terminal_ip}</Table.Cell>
                  <Table.Cell>
                    <Chip color="accent" variant="soft" size="sm">
                      {row.channel_text}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell>{row.stage_text}</Table.Cell>
                  <Table.Cell>{row.method}</Table.Cell>
                  <Table.Cell className="max-w-64 truncate font-mono text-xs">
                    {row.target}
                  </Table.Cell>
                  <Table.Cell>
                    {row.http_status == null ? (
                      "—"
                    ) : (
                      <Chip size="sm" color="accent" variant="soft">
                        {row.http_status}
                      </Chip>
                    )}
                  </Table.Cell>
                  <Table.Cell>
                    <span
                      className={
                        row.ui_tone === "danger"
                          ? "text-[var(--telemetry-danger)]"
                          : row.ui_tone === "positive"
                            ? "text-[var(--telemetry-good)]"
                            : ""
                      }
                    >
                      {row.result}
                    </span>
                  </Table.Cell>
                  <Table.Cell>{formatDuration(row.duration_ms)}</Table.Cell>
                  <Table.Cell>{row.matched_rule_ids.length}</Table.Cell>
                  <Table.Cell>{formatBytes(row.size_bytes)}</Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
        <Table.Footer className="flex items-center justify-between px-4 py-3 text-sm">
          <span>
            当前显示 {props.page.data?.rows.length ?? 0} 条，共{" "}
            {props.page.data?.total ?? 0} 条
          </span>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              isDisabled={(props.page.data?.page ?? 1) <= 1}
              onPress={() =>
                props.setQuery((query) => ({
                  ...query,
                  page: {
                    ...query.page,
                    page: Math.max(1, query.page.page - 1),
                  },
                }))
              }
            >
              上一页
            </Button>
            <span>
              {props.page.data?.page ?? 1} / {props.page.data?.total_pages ?? 1}
            </span>
            <Button
              size="sm"
              variant="outline"
              isDisabled={
                (props.page.data?.page ?? 1) >=
                (props.page.data?.total_pages ?? 1)
              }
              onPress={() =>
                props.setQuery((query) => ({
                  ...query,
                  page: { ...query.page, page: query.page.page + 1 },
                }))
              }
            >
              下一页
            </Button>
          </div>
          {props.page.isLoading && <Spinner size="sm" />}
        </Table.Footer>
      </Table>
    </div>
  );
}
