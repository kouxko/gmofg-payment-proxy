"use client";

/**
 * 会话历史页面。
 *
 * Rust 内存仓库负责会话容量、筛选、排序和分页；这里保存查询表单、选中会话、
 * Overlay 开关与确认状态。完整 Payload 按需读取，关闭详情时会失效并释放引用。
 */

import { useMemo, useState } from "react";
import {
  Alert,
  AlertDialog,
  Button,
  Calendar,
  Card,
  Chip,
  DateField,
  DatePicker,
  Drawer,
  Input,
  Label,
  ListBox,
  SearchField,
  Select,
  Spinner,
  Table,
  Tabs,
  toast,
} from "@heroui/react";
import {
  parseAbsoluteToLocal,
  parseDateTime,
  toCalendarDateTime,
  type DateValue,
} from "@internationalized/date";
import { ArrowDownToLine, Eye, TrashBin } from "@gravity-ui/icons";
import type {
  SessionDetailViewModel,
  SessionPageViewModel,
  SessionQuery,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import {
  formatBytes,
  formatDuration,
  formatTimestamp,
  toneColor,
} from "@/lib/format";

export const defaultSessionQuery: SessionQuery = {
  keyword: null,
  terminal_ip: null,
  channel: null,
  result: null,
  rule_id: null,
  started_from: null,
  started_to: null,
  sort: "started_at",
  direction: "desc",
  page: { page: 1, page_size: 10 },
};

export const sessionDetailTabLabels = {
  overview: "概览",
  request: "请求",
  response: "响应",
} as const;

export function sessionFilterDateValue(value: string | null): DateValue | null {
  // 同时兼容带时区和不带时区的 Rust/旧草稿文本；解析失败显示为空，由 Rust
  // 在提交查询时继续承担最终合法性判断。
  if (!value) return null;
  try {
    return parseAbsoluteToLocal(value);
  } catch {
    try {
      return parseDateTime(value);
    } catch {
      return null;
    }
  }
}

export function sessionFilterDateText(value: DateValue | null): string | null {
  // IPC 约定保留到分钟，秒和毫秒不会由前端凭空补齐。
  if (!value) return null;
  return toCalendarDateTime(value).toString().slice(0, 16);
}

export function SessionsView() {
  const { bootstrap } = useBootstrap();
  const channelCatalog = bootstrap?.channel_catalog ?? [];
  const [selectedId, setSelectedId] = useState<string>();
  const [query, setQuery] = useState(defaultSessionQuery);
  const [detailOpen, setDetailOpen] = useState(false);
  const [detailRequested, setDetailRequested] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportPending, setExportPending] = useState(false);
  const [clearDialogOpen, setClearDialogOpen] = useState(false);
  const [clearPending, setClearPending] = useState(false);
  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const page = useIpcQuery<SessionPageViewModel>(`session-query:${queryKey}`, () =>
    callCommand(commands.sessionQuery(query)),
  );
  useAppEventRefresh(["session_updated", "snapshot_required"], page.refresh);
  const detail = useIpcQuery<SessionDetailViewModel>(
    `session-detail:${detailRequested ? selectedId ?? "none" : "closed"}`,
    () => callCommand(commands.sessionGet(selectedId!)),
    undefined,
    { enabled: Boolean(selectedId && detailRequested) },
  );
  // detailRequested 将“选中列表行”和“确实需要 Payload”分开，避免列表浏览时
  // 自动把大量敏感报文加载进 WebView。
  useAppEventRefresh(["session_updated"], detail.refresh, {
    paused: !selectedId || !detailRequested,
    entityId: selectedId,
  });
  const selected = page.data?.items.find(
    (item) => item.session_id === selectedId,
  );

  async function exportSelected() {
    // Rust 打开系统保存对话框并写文件；前端只负责敏感数据二次确认。
    if (!selectedId || exportPending) return;
    setExportPending(true);
    try {
      const result = await callCommand(commands.sessionExport(selectedId, true));
      toast(result.message, { variant: toneColor(result.ui_tone) });
      setExportDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setExportPending(false);
    }
  }

  async function clearSessions() {
    // 清空成功后同时关闭详情并让旧 Promise/引用失效，避免展示已删除数据。
    if (clearPending) return;
    setClearPending(true);
    try {
      const result = await callCommand(commands.sessionClear(true));
      toast(result.message, { variant: toneColor(result.ui_tone) });
      setDetailOpen(false);
      setDetailRequested(false);
      setSelectedId(undefined);
      detail.invalidate();
      await page.refresh();
      setClearDialogOpen(false);
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setClearPending(false);
    }
  }

  return (
    <section className="grid h-full grid-cols-[minmax(0,1fr)_380px] max-[1280px]:grid-cols-1">
      <div className="min-w-0 space-y-4 overflow-auto p-5">
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
                value={query.keyword ?? ""}
                onChange={(keyword) =>
                  setQuery({
                    ...query,
                    keyword: keyword || null,
                    page: { ...query.page, page: 1 },
                  })
                }
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
                value={query.terminal_ip ?? ""}
                onChange={(event) =>
                  setQuery({
                    ...query,
                    terminal_ip: event.target.value || null,
                    page: { ...query.page, page: 1 },
                  })
                }
              />
            </div>
            <div className="grid min-w-0 gap-1">
              <Label>通道</Label>
              <Select
                aria-label="通道筛选"
                selectedKey={query.channel ?? "all"}
                onSelectionChange={(key) =>
                  setQuery({
                    ...query,
                    channel:
                      key === "all" ? null : (key as SessionQuery["channel"]),
                    page: { ...query.page, page: 1 },
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
                    {channelCatalog.map((channel) => (
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
                value={query.result ?? ""}
                onChange={(event) =>
                  setQuery({
                    ...query,
                    result: event.target.value || null,
                    page: { ...query.page, page: 1 },
                  })
                }
              />
            </div>
            <div className="grid min-w-0 gap-1">
              <Label>规则 ID</Label>
              <Input
                aria-label="规则 ID 筛选"
                placeholder="规则 ID"
                value={query.rule_id ?? ""}
                onChange={(event) =>
                  setQuery({
                    ...query,
                    rule_id: event.target.value || null,
                    page: { ...query.page, page: 1 },
                  })
                }
              />
            </div>
            <DatePicker
              className="min-w-0"
              granularity="minute"
              hourCycle={24}
              hideTimeZone
              value={sessionFilterDateValue(query.started_from)}
              onChange={(value) =>
                setQuery({
                  ...query,
                  started_from: sessionFilterDateText(value),
                  page: { ...query.page, page: 1 },
                })
              }
            >
              <Label>开始时间</Label>
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
                <Calendar aria-label="选择开始日期">
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
            <DatePicker
              className="min-w-0"
              granularity="minute"
              hourCycle={24}
              hideTimeZone
              value={sessionFilterDateValue(query.started_to)}
              onChange={(value) =>
                setQuery({
                  ...query,
                  started_to: sessionFilterDateText(value),
                  page: { ...query.page, page: 1 },
                })
              }
            >
              <Label>结束时间</Label>
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
                <Calendar aria-label="选择结束日期">
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
            <Button
              className="self-end"
              variant="primary"
              onPress={() => void page.refresh()}
            >
              应用筛选
            </Button>
          </Card.Content>
        </Card>

        {page.error && <Alert status="danger">{page.error}</Alert>}
        <Table>
          <Table.ScrollContainer>
            <Table.Content
              aria-label="会话记录"
              className="min-w-[1080px]"
              selectionMode="single"
              selectedKeys={selectedId ? [selectedId] : []}
              onSelectionChange={(keys) => {
                if (keys === "all") return;
                const first = Array.from(keys)[0];
                if (first != null) {
                  setSelectedId(String(first));
                  detail.invalidate();
                  setDetailRequested(
                    window.matchMedia("(min-width: 1281px)").matches,
                  );
                }
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
                    {page.isLoading
                      ? "正在查询会话…"
                      : page.error
                        ? "会话列表暂不可用"
                        : (page.data?.empty_message ?? "暂无会话记录")}
                  </div>
                )}
              >
                {(page.data?.items ?? []).map((session) => (
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
              共 {page.data?.total ?? 0} 条 / {page.data?.page_size ?? 10} 条每页
            </span>
            <div
              className="ml-auto flex items-center gap-3"
              aria-label="会话分页"
            >
              <Button
                size="sm"
                variant="outline"
                isDisabled={(page.data?.page ?? 1) <= 1}
                onPress={() =>
                  setQuery({
                    ...query,
                    page: {
                      ...query.page,
                      page: Math.max(1, query.page.page - 1),
                    },
                  })
                }
              >
                上一页
              </Button>
              <span className="tabular-nums">
                {page.data?.page ?? 1} / {page.data?.total_pages ?? 1}
              </span>
              <Button
                size="sm"
                variant="outline"
                isDisabled={
                  (page.data?.page ?? 1) >= (page.data?.total_pages ?? 1)
                }
                onPress={() =>
                  setQuery({
                    ...query,
                    page: { ...query.page, page: query.page.page + 1 },
                  })
                }
              >
                下一页
              </Button>
            </div>
          </Table.Footer>
        </Table>

        <div className="flex items-center gap-3">
          <Drawer
            isOpen={detailOpen}
            onOpenChange={(open) => {
              setDetailOpen(open);
              setDetailRequested(open);
              if (!open) detail.invalidate();
            }}
          >
            <Button
              isDisabled={!selectedId}
              variant="outline"
            >
              <Eye className="size-4" />
              查看完整报文
            </Button>
            <Drawer.Backdrop>
              <Drawer.Content placement="right">
                <Drawer.Dialog>
                  <Drawer.Header>
                    <Drawer.Heading>完整会话报文</Drawer.Heading>
                  </Drawer.Header>
                  <Drawer.Body className="space-y-5">
                    {detail.isLoading && (
                      <div className="grid min-h-40 place-items-center">
                        <Spinner aria-label="正在读取完整会话报文" />
                      </div>
                    )}
                    {detail.error && (
                      <Alert status="danger">
                        <Alert.Indicator />
                        <Alert.Content>
                          <Alert.Title>读取会话详情失败</Alert.Title>
                          <Alert.Description>{detail.error}</Alert.Description>
                        </Alert.Content>
                        <Button
                          size="sm"
                          variant="outline"
                          onPress={() => void detail.refresh()}
                        >
                          重试
                        </Button>
                      </Alert>
                    )}
                    {detail.data && (
                      <>
                        <div className="space-y-3">
                          <h3 className="font-semibold">请求 Header</h3>
                          <Table>
                            <Table.ScrollContainer>
                              <Table.Content aria-label="会话请求 HTTP Header">
                                <Table.Header>
                                  <Table.Column isRowHeader>名称</Table.Column>
                                  <Table.Column>值</Table.Column>
                                </Table.Header>
                                <Table.Body
                                  renderEmptyState={() => (
                                    <div className="p-4 text-center text-sm text-[var(--telemetry-muted)]">
                                      无请求 Header
                                    </div>
                                  )}
                                >
                                  {Object.entries(
                                    detail.data.request?.headers ?? {},
                                  ).flatMap(([name, values]) =>
                                    values.map((value, index) => (
                                      <Table.Row key={`${name}-${index}`}>
                                        <Table.Cell className="font-mono text-xs">
                                          {name}
                                        </Table.Cell>
                                        <Table.Cell className="break-all font-mono text-xs">
                                          {value}
                                        </Table.Cell>
                                      </Table.Row>
                                    )),
                                  )}
                                </Table.Body>
                              </Table.Content>
                            </Table.ScrollContainer>
                          </Table>
                        </div>
                        <div>
                          <h3 className="mb-2 font-semibold">请求 Body</h3>
                          <pre className="whitespace-pre-wrap break-all text-xs">
                            {detail.data.request?.body_text ?? "无请求正文"}
                          </pre>
                        </div>
                        <div>
                          <h3 className="mb-2 font-semibold">HTTP 状态码</h3>
                          <Chip size="sm" color="accent" variant="soft">
                            {detail.data.response?.http_status ?? "无响应"}
                          </Chip>
                        </div>
                        <div className="space-y-3">
                          <h3 className="font-semibold">响应 Header</h3>
                          <Table>
                            <Table.ScrollContainer>
                              <Table.Content aria-label="会话响应 HTTP Header">
                                <Table.Header>
                                  <Table.Column isRowHeader>名称</Table.Column>
                                  <Table.Column>值</Table.Column>
                                </Table.Header>
                                <Table.Body
                                  renderEmptyState={() => (
                                    <div className="p-4 text-center text-sm text-[var(--telemetry-muted)]">
                                      无响应 Header
                                    </div>
                                  )}
                                >
                                  {Object.entries(
                                    detail.data.response?.headers ?? {},
                                  ).flatMap(([name, values]) =>
                                    values.map((value, index) => (
                                      <Table.Row key={`${name}-${index}`}>
                                        <Table.Cell className="font-mono text-xs">
                                          {name}
                                        </Table.Cell>
                                        <Table.Cell className="break-all font-mono text-xs">
                                          {value}
                                        </Table.Cell>
                                      </Table.Row>
                                    )),
                                  )}
                                </Table.Body>
                              </Table.Content>
                            </Table.ScrollContainer>
                          </Table>
                        </div>
                        <div>
                          <h3 className="mb-2 font-semibold">响应 Body</h3>
                          <pre className="whitespace-pre-wrap break-all text-xs">
                            {detail.data.response?.body_text ?? "无响应正文"}
                          </pre>
                        </div>
                      </>
                    )}
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
          {selectedId ? (
            <AlertDialog
              isOpen={exportDialogOpen}
              onOpenChange={(open) => {
                if (!open && exportPending) return;
                setExportDialogOpen(open);
              }}
            >
              <Button variant="outline">
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
                      导出的 JSON 文件包含原始敏感数据。保存位置和文件写入均由
                      Rust 原生侧处理。
                    </AlertDialog.Body>
                    <AlertDialog.Footer>
                      <Button
                        slot="close"
                        variant="outline"
                        isDisabled={exportPending}
                      >
                        取消
                      </Button>
                      <Button
                        variant="primary"
                        isDisabled={exportPending}
                        onPress={() => void exportSelected()}
                      >
                        {exportPending ? "正在导出…" : "确认并选择位置"}
                      </Button>
                    </AlertDialog.Footer>
                  </AlertDialog.Dialog>
                </AlertDialog.Container>
              </AlertDialog.Backdrop>
            </AlertDialog>
          ) : (
            <Button variant="outline" isDisabled>
              <ArrowDownToLine className="size-4" />
              导出所选会话
            </Button>
          )}
          <AlertDialog
            isOpen={clearDialogOpen}
            onOpenChange={(open) => {
              if (!open && clearPending) return;
              setClearDialogOpen(open);
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
                      isDisabled={clearPending}
                    >
                      取消
                    </Button>
                    <Button
                      variant="danger"
                      isDisabled={clearPending}
                      onPress={() => void clearSessions()}
                    >
                      {clearPending ? "正在清空…" : "确认清空"}
                    </Button>
                  </AlertDialog.Footer>
                </AlertDialog.Dialog>
              </AlertDialog.Container>
            </AlertDialog.Backdrop>
          </AlertDialog>
        </div>
      </div>

      <aside className="hidden min-w-0 overflow-auto border-l border-[var(--telemetry-line)] p-4 min-[1281px]:block">
        {selectedId && (
          <Button
            className="mb-3 ml-auto"
            size="sm"
            variant="ghost"
            onPress={() => {
              setSelectedId(undefined);
              setDetailRequested(false);
              detail.invalidate();
            }}
          >
            关闭详情并释放报文
          </Button>
        )}
        {selectedId && detail.error && (
          <Alert status="danger" className="mb-4">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>读取会话详情失败</Alert.Title>
              <Alert.Description>{detail.error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void detail.refresh()}
            >
              重试
            </Button>
          </Alert>
        )}
        <Tabs defaultSelectedKey="overview">
          <Tabs.ListContainer>
            <Tabs.List aria-label="会话详情">
              <Tabs.Tab id="overview">
                {sessionDetailTabLabels.overview}
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="request">
                {sessionDetailTabLabels.request}
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="response">
                {sessionDetailTabLabels.response}
                <Tabs.Indicator />
              </Tabs.Tab>
            </Tabs.List>
          </Tabs.ListContainer>
          <Tabs.Panel id="overview" className="pt-5">
            {!selected ? (
              <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
                选择会话查看完整概览
              </p>
            ) : detail.isLoading ? (
              <div className="grid min-h-40 place-items-center">
                <Spinner aria-label="正在读取会话详情" />
              </div>
            ) : (
              <div className="space-y-5">
                <dl className="grid grid-cols-[112px_1fr] gap-y-3 text-sm">
                  <dt>请求 ID</dt>
                  <dd className="break-all font-mono text-xs">{selected.request_id}</dd>
                  <dt>终端证书指纹</dt>
                  <dd className="break-all">{detail.data?.certificate_fingerprint ?? "正在读取…"}</dd>
                  <dt>上游主机</dt>
                  <dd>{detail.data?.upstream_host ?? "—"}</dd>
                  <dt>通道</dt>
                  <dd>{selected.channel_text}</dd>
                  <dt>结果</dt>
                  <dd>{selected.result}</dd>
                  <dt>最终动作</dt>
                  <dd>{detail.data?.final_action ?? "—"}</dd>
                  <dt>客户端 → 代理</dt>
                  <dd>{detail.data?.app_to_proxy_tls ?? "—"}</dd>
                  <dt>代理 → 上游</dt>
                  <dd>{detail.data?.proxy_to_server_tls ?? "—"}</dd>
                </dl>
                {Object.keys(detail.data?.extracted_metadata ?? {}).length > 0 && (
                  <div>
                    <h2 className="mb-2 font-semibold">Workspace 提取结果</h2>
                    <dl className="grid grid-cols-[112px_1fr] gap-y-2 text-sm">
                      {Object.entries(detail.data?.extracted_metadata ?? {}).map(([name, value]) => (
                        <div key={name} className="contents"><dt>{name}</dt><dd className="break-all font-mono text-xs">{value}</dd></div>
                      ))}
                    </dl>
                  </div>
                )}
                {(detail.data?.response_assertions?.length ?? 0) > 0 && (
                  <div>
                    <h2 className="mb-2 font-semibold">响应断言</h2>
                    <ul className="space-y-2 text-sm">
                      {(detail.data?.response_assertions ?? []).map((assertion) => (
                        <li key={assertion.assertion_id} className="flex items-start gap-2">
                          <Chip size="sm" color={assertion.passed ? "success" : "danger"} variant="soft">{assertion.passed ? "通过" : "失败"}</Chip>
                          <span><strong>{assertion.name}</strong><br /><span className="text-xs text-[var(--telemetry-muted)]">{assertion.message}</span></span>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                <div>
                  <h2 className="mb-2 font-semibold">规则轨迹</h2>
                  <div className="space-y-2 text-sm">
                    {(detail.data?.rule_trace ?? []).map((entry, index) => (
                      <div key={`${entry}-${index}`}>{entry}</div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </Tabs.Panel>
          <Tabs.Panel id="request" className="pt-4">
            <div className="space-y-4">
              <h2 className="font-semibold">请求 Header</h2>
              <Table>
                <Table.ScrollContainer>
                  <Table.Content aria-label="详情请求 HTTP Header">
                    <Table.Header>
                      <Table.Column isRowHeader>名称</Table.Column>
                      <Table.Column>值</Table.Column>
                    </Table.Header>
                    <Table.Body renderEmptyState={() => <div className="p-4 text-center text-sm text-[var(--telemetry-muted)]">无请求 Header</div>}>
                      {Object.entries(detail.data?.request?.headers ?? {}).flatMap(([name, values]) =>
                        values.map((value, index) => (
                          <Table.Row key={`${name}-${index}`}>
                            <Table.Cell className="font-mono text-xs">{name}</Table.Cell>
                            <Table.Cell className="break-all font-mono text-xs">{value}</Table.Cell>
                          </Table.Row>
                        )),
                      )}
                    </Table.Body>
                  </Table.Content>
                </Table.ScrollContainer>
              </Table>
              <h2 className="font-semibold">请求 Body</h2>
              <pre className="whitespace-pre-wrap break-all text-xs">{detail.data?.request?.body_text ?? "按需读取后显示请求报文"}</pre>
            </div>
          </Tabs.Panel>
          <Tabs.Panel id="response" className="pt-4">
            <div className="space-y-4">
              <h2 className="font-semibold">响应 Header</h2>
              <Table>
                <Table.ScrollContainer>
                  <Table.Content aria-label="详情响应 HTTP Header">
                    <Table.Header>
                      <Table.Column isRowHeader>名称</Table.Column>
                      <Table.Column>值</Table.Column>
                    </Table.Header>
                    <Table.Body renderEmptyState={() => <div className="p-4 text-center text-sm text-[var(--telemetry-muted)]">无响应 Header</div>}>
                      {Object.entries(detail.data?.response?.headers ?? {}).flatMap(([name, values]) =>
                        values.map((value, index) => (
                          <Table.Row key={`${name}-${index}`}>
                            <Table.Cell className="font-mono text-xs">{name}</Table.Cell>
                            <Table.Cell className="break-all font-mono text-xs">{value}</Table.Cell>
                          </Table.Row>
                        )),
                      )}
                    </Table.Body>
                  </Table.Content>
                </Table.ScrollContainer>
              </Table>
              <h2 className="font-semibold">响应 Body</h2>
              <pre className="whitespace-pre-wrap break-all text-xs">{detail.data?.response?.body_text ?? "按需读取后显示响应报文"}</pre>
            </div>
          </Tabs.Panel>
        </Tabs>
      </aside>
    </section>
  );
}
