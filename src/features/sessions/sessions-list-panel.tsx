import type { Dispatch, SetStateAction } from "react";
import {
  Alert,
  Button,
  Calendar,
  Card,
  Chip,
  DateField,
  DatePicker,
  Input,
  Label,
  ListBox,
  SearchField,
  Select,
  Table,
} from "@heroui/react";
import type {
  ChannelPresentationViewModel,
  SessionPageViewModel,
  SessionQuery,
} from "@/generated/rust-types";
import {
  formatBytes,
  formatDuration,
  formatTimestamp,
  toneColor,
} from "@/lib/format";
import {
  sessionFilterDateText,
  sessionFilterDateValue,
} from "./session-config";

interface SessionsListPanelProps {
  query: SessionQuery;
  setQuery: Dispatch<SetStateAction<SessionQuery>>;
  page: {
    data?: SessionPageViewModel;
    error?: string;
    isLoading: boolean;
    refresh: () => Promise<void>;
  };
  channels: ChannelPresentationViewModel[];
  selectedId?: string;
  onSelect: (id: string) => void;
}

function DateFilter({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  return (
    <DatePicker
      className="min-w-0"
      granularity="minute"
      hourCycle={24}
      hideTimeZone
      value={sessionFilterDateValue(value)}
      onChange={(next) => onChange(sessionFilterDateText(next))}
    >
      <Label>{label}</Label>
      <DateField.Group fullWidth>
        <DateField.Input>
          {(segment) => <DateField.Segment segment={segment} />}
        </DateField.Input>
        <DateField.Suffix>
          <DatePicker.Trigger>
            <DatePicker.TriggerIndicator />
          </DatePicker.Trigger>
        </DateField.Suffix>
      </DateField.Group>
      <DatePicker.Popover>
        <Calendar aria-label={`选择${label}`}>
          <Calendar.Header>
            <Calendar.Heading />
            <Calendar.NavButton slot="previous" />
            <Calendar.NavButton slot="next" />
          </Calendar.Header>
          <Calendar.Grid>
            <Calendar.GridHeader>
              {(day) => <Calendar.HeaderCell>{day}</Calendar.HeaderCell>}
            </Calendar.GridHeader>
            <Calendar.GridBody>
              {(date) => <Calendar.Cell date={date} />}
            </Calendar.GridBody>
          </Calendar.Grid>
        </Calendar>
      </DatePicker.Popover>
    </DatePicker>
  );
}

export function SessionsListPanel(props: SessionsListPanelProps) {
  const update = (change: Partial<SessionQuery>) =>
    props.setQuery((query) => ({
      ...query,
      ...change,
      page: { ...query.page, page: 1 },
    }));
  return (
    <>
      <div className="flex items-center gap-3">
        <h1 className="text-2xl font-semibold">会话记录</h1>
        <Chip color="warning" variant="soft">
          报文仅保存在内存中，重启后清空
        </Chip>
      </div>
      <Card>
        <Card.Content className="grid grid-cols-3 gap-3 p-4 max-[900px]:grid-cols-2">
          <div className="col-span-2 grid min-w-0 gap-1">
            <Label>关键字或请求 ID</Label>
            <SearchField
              aria-label="关键字或请求 ID"
              value={props.query.keyword ?? ""}
              onChange={(keyword) => update({ keyword: keyword || null })}
            >
              <SearchField.Group>
                <SearchField.SearchIcon />
                <SearchField.Input placeholder="请求 ID / 路径 / 主机 / 关键字" />
                <SearchField.ClearButton />
              </SearchField.Group>
            </SearchField>
          </div>
          <div className="grid min-w-0 gap-1">
            <Label>终端 IP</Label>
            <Input
              aria-label="终端 IP"
              placeholder="例如 192.168.1.20"
              value={props.query.terminal_ip ?? ""}
              onChange={(event) =>
                update({ terminal_ip: event.target.value || null })
              }
            />
          </div>
          <div className="grid min-w-0 gap-1">
            <Label>通道</Label>
            <Select
              aria-label="通道筛选"
              selectedKey={props.query.channel ?? "all"}
              onSelectionChange={(key) =>
                update({
                  channel:
                    key === "all" ? null : (key as SessionQuery["channel"]),
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
            <Label>结果</Label>
            <Input
              aria-label="结果筛选"
              placeholder="结果"
              value={props.query.result ?? ""}
              onChange={(event) =>
                update({ result: event.target.value || null })
              }
            />
          </div>
          <div className="grid min-w-0 gap-1">
            <Label>规则 ID</Label>
            <Input
              aria-label="规则 ID 筛选"
              placeholder="规则 ID"
              value={props.query.rule_id ?? ""}
              onChange={(event) =>
                update({ rule_id: event.target.value || null })
              }
            />
          </div>
          <DateFilter
            label="开始时间"
            value={props.query.started_from}
            onChange={(started_from) => update({ started_from })}
          />
          <DateFilter
            label="结束时间"
            value={props.query.started_to}
            onChange={(started_to) => update({ started_to })}
          />
          <Button
            className="self-end"
            variant="primary"
            onPress={() => void props.page.refresh()}
          >
            应用筛选
          </Button>
        </Card.Content>
      </Card>
      {props.page.error && <Alert status="danger">{props.page.error}</Alert>}
      <Table>
        <Table.ScrollContainer>
          <Table.Content
            aria-label="会话记录"
            className="min-w-[1080px]"
            selectionMode="single"
            selectedKeys={props.selectedId ? [props.selectedId] : []}
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              const first = Array.from(keys)[0];
              if (first != null) props.onSelect(String(first));
            }}
          >
            <Table.Header>
              <Table.Column isRowHeader>时间</Table.Column>
              <Table.Column>终端 IP</Table.Column>
              <Table.Column>通道</Table.Column>
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
                    ? "正在查询会话…"
                    : props.page.error
                      ? "会话列表暂不可用"
                      : (props.page.data?.empty_message ?? "暂无会话记录")}
                </div>
              )}
            >
              {(props.page.data?.items ?? []).map((session) => (
                <Table.Row key={session.session_id} id={session.session_id}>
                  <Table.Cell className="whitespace-nowrap">
                    {formatTimestamp(session.started_at)}
                  </Table.Cell>
                  <Table.Cell>{session.terminal_ip}</Table.Cell>
                  <Table.Cell>
                    <Chip size="sm" color="accent" variant="soft">
                      {session.channel_text}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell>{session.method}</Table.Cell>
                  <Table.Cell className="max-w-64 truncate font-mono text-xs">
                    {session.target}
                  </Table.Cell>
                  <Table.Cell>
                    {session.http_status == null ? (
                      "—"
                    ) : (
                      <Chip size="sm" color="accent" variant="soft">
                        {session.http_status}
                      </Chip>
                    )}
                  </Table.Cell>
                  <Table.Cell>
                    <Chip
                      size="sm"
                      color={toneColor(session.ui_tone)}
                      variant="soft"
                    >
                      {session.result}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell>{formatDuration(session.duration_ms)}</Table.Cell>
                  <Table.Cell>{session.matched_rule_ids.length}</Table.Cell>
                  <Table.Cell className="whitespace-nowrap">
                    {formatBytes(session.request_size_bytes)} /{" "}
                    {formatBytes(session.response_size_bytes)}
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
        <Table.Footer className="flex items-center gap-4 px-4 py-3">
          <span className="text-sm">
            共 {props.page.data?.total ?? 0} 条 /{" "}
            {props.page.data?.page_size ?? 10} 条每页
          </span>
          <div
            className="ml-auto flex items-center gap-3"
            aria-label="会话分页"
          >
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
            <span className="tabular-nums">
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
        </Table.Footer>
      </Table>
    </>
  );
}
