"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Chip,
  Input,
  Label,
  SearchField,
  Select,
  ListBox,
  Spinner,
  Table,
  Tabs,
  TextField,
  toast,
} from "@heroui/react";
import { Circle, Copy, Pause, Play, TrashBin } from "@gravity-ui/icons";
import type {
  CaptureDetailViewModel,
  CapturePageViewModel,
  CaptureQuery,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { formatBytes, formatDuration, formatTimestamp, toneColor } from "@/lib/format";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";
import {
  useAppEventRefresh,
  useBootstrap,
} from "@/features/shell/bootstrap-context";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";

export const defaultCaptureQuery: CaptureQuery = {
  keyword: null,
  terminal_ip: null,
  channel: null,
  stage: null,
  result: null,
  rule_id: null,
  after_event_id: null,
  sort: "occurred_at",
  direction: "desc",
  page: { page: 1, page_size: 50 },
};

export function ruleEditorHref(sessionId: string): string {
  return `/rules?sessionId=${encodeURIComponent(sessionId)}`;
}

export function resumeCaptureQuery(query: CaptureQuery): CaptureQuery {
  return {
    ...query,
    after_event_id: null,
    page: { ...query.page, page: 1 },
  };
}

export function CaptureView({
  initialPage,
}: {
  initialPage?: CapturePageViewModel;
}) {
  const { navigate } = useWorkspaceNavigation();
  const { proxy } = useBootstrap();
  const [paused, setPaused] = useState(false);
  const [clearPending, setClearPending] = useState(false);
  const [selectedEventId, setSelectedEventId] = useState<number>();
  const detailPanelRef = useRef<HTMLElement>(null);
  const [query, setQuery] = useState(defaultCaptureQuery);
  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const page = useIpcQuery<CapturePageViewModel>(
    `capture-query:${queryKey}`,
    () => callCommand(commands.captureQuery(query)),
    initialPage,
  );
  useAppEventRefresh(
    ["capture_rows_added", "snapshot_required"],
    page.refresh,
    { paused },
  );
  useEffect(() => {
    if (!page.data?.snapshot_required || query.after_event_id == null) return;
    const task = window.setTimeout(() => {
      setQuery((current) => ({
        ...current,
        after_event_id: null,
        page: { ...current.page, page: 1 },
      }));
    }, 0);
    return () => window.clearTimeout(task);
  }, [page.data?.snapshot_required, query.after_event_id]);
  const selected = page.data?.rows.find(
    (row) => row.event_id === selectedEventId,
  );
  const selectedId = selected?.session_id;
  const detail = useIpcQuery<CaptureDetailViewModel>(
    `capture-detail:${selectedId ?? "none"}`,
    () =>
      callCommand(
        commands.captureGetDetail(
          selected!.session_id,
          selected!.runtime_epoch,
        ),
      ),
    undefined,
    { enabled: Boolean(selected) },
  );

  async function clearCurrentView() {
    if (!page.data || clearPending) return;
    setClearPending(true);
    try {
      await callCommand(commands.captureClearView(page.data.event_cursor));
      setSelectedEventId(undefined);
      detail.invalidate();
      await page.refresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setClearPending(false);
    }
  }

  function createRuleFromSession() {
    if (!selectedId) return;
    navigate(ruleEditorHref(selectedId));
  }

  function togglePaused() {
    if (!paused) {
      setPaused(true);
      return;
    }
    setPaused(false);
    setQuery((current) => resumeCaptureQuery(current));
  }

  function selectEvent(eventId: number) {
    setSelectedEventId(eventId);
    if (window.matchMedia("(max-width: 1280px)").matches) {
      requestAnimationFrame(() => {
        detailPanelRef.current?.scrollIntoView({ block: "start" });
      });
    }
  }

  return (
    <section className="grid h-full grid-cols-[minmax(0,1fr)_380px] max-[1280px]:grid-cols-1">
      <div className="min-w-0 space-y-4 overflow-auto p-5">
        <div className="flex items-start">
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-2xl font-semibold">实时抓包</h1>
              <Chip
                color={proxy ? toneColor(proxy.ui_tone) : "default"}
                size="sm"
                variant="soft"
              >
                {proxy?.state_text ?? "正在读取代理状态"}
              </Chip>
            </div>
            <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
              暂停列表滚动不会影响网络转发、规则或会话记录
            </p>
          </div>
          <div className="ml-auto flex gap-2">
            <Button variant="outline" onPress={togglePaused}>
              {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
              {paused ? "恢复列表滚动" : "暂停列表滚动"}
            </Button>
            <Button
              variant="danger-soft"
              isDisabled={clearPending}
              onPress={() => void clearCurrentView()}
            >
              <TrashBin className="size-4" />
              {clearPending ? "正在清空…" : "清空当前显示"}
            </Button>
          </div>
        </div>

        <Card>
          <Card.Content className="grid grid-cols-[minmax(210px,2fr)_minmax(140px,1fr)_minmax(150px,1fr)_minmax(150px,1fr)] gap-3 p-4 max-[900px]:grid-cols-2">
            <div className="grid min-w-0 gap-1">
              <Label>关键字或请求 ID</Label>
              <SearchField
                aria-label="关键字或请求 ID"
                value={query.keyword ?? ""}
                onChange={(keyword) =>
                  setQuery({
                    ...query,
                    keyword: keyword || null,
                    after_event_id: null,
                    page: { ...query.page, page: 1 },
                  })
                }
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
                value={query.terminal_ip ?? ""}
                onChange={(event) =>
                  setQuery({
                    ...query,
                    terminal_ip: event.target.value || null,
                    after_event_id: null,
                    page: { ...query.page, page: 1 },
                  })
                }
              />
            </TextField>
            <div className="grid min-w-0 gap-1">
              <Label>通道</Label>
              <Select
                aria-label="通道筛选"
                selectedKey={query.channel ?? "all"}
                onSelectionChange={(key) =>
                  setQuery({
                    ...query,
                    channel:
                      key === "all" ? null : (key as CaptureQuery["channel"]),
                    after_event_id: null,
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
                    <ListBox.Item id="all">全部通道</ListBox.Item>
                    <ListBox.Item id="transaction">交易</ListBox.Item>
                    <ListBox.Item id="dll">DLL</ListBox.Item>
                  </ListBox>
                </Select.Popover>
              </Select>
            </div>
            <div className="grid min-w-0 gap-1">
              <Label>阶段</Label>
              <Select
                aria-label="阶段筛选"
                selectedKey={query.stage ?? "all"}
                onSelectionChange={(key) =>
                  setQuery({
                    ...query,
                    stage:
                      key === "all" ? null : (key as CaptureQuery["stage"]),
                    after_event_id: null,
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
                value={query.result ?? ""}
                onChange={(event) =>
                  setQuery({
                    ...query,
                    result: event.target.value || null,
                    after_event_id: null,
                    page: { ...query.page, page: 1 },
                  })
                }
              />
            </TextField>
            <TextField>
              <Label>规则 ID</Label>
              <Input
                placeholder="命中的规则 ID"
                value={query.rule_id ?? ""}
                onChange={(event) =>
                  setQuery({
                    ...query,
                    rule_id: event.target.value || null,
                    after_event_id: null,
                    page: { ...query.page, page: 1 },
                  })
                }
              />
            </TextField>
          </Card.Content>
        </Card>

        {page.error && (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>抓包列表读取失败</Alert.Title>
              <Alert.Description>{page.error}</Alert.Description>
            </Alert.Content>
            <Button
              size="sm"
              variant="outline"
              onPress={() => void page.refresh()}
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
              selectedKeys={selectedEventId != null ? [String(selectedEventId)] : []}
              onSelectionChange={(keys) => {
                if (keys === "all") return;
                const first = Array.from(keys)[0];
                if (first != null) selectEvent(Number(first));
              }}
            >
              <Table.Header>
                <Table.Column isRowHeader>时间（毫秒）</Table.Column>
                <Table.Column>终端 IP</Table.Column>
                <Table.Column>通道</Table.Column>
                <Table.Column>方向</Table.Column>
                <Table.Column>方法</Table.Column>
                <Table.Column>路径 / 请求类型</Table.Column>
                <Table.Column>结果</Table.Column>
                <Table.Column>耗时</Table.Column>
                <Table.Column>匹配规则</Table.Column>
                <Table.Column>大小</Table.Column>
              </Table.Header>
              <Table.Body
                renderEmptyState={() => (
                  <div className="p-8 text-center">
                    {page.isLoading ? "正在查询…" : page.data?.empty_message}
                  </div>
                )}
              >
                {(page.data?.rows ?? []).map((row) => (
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
            <span>当前显示 {page.data?.rows.length ?? 0} 条，共 {page.data?.total ?? 0} 条</span>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                isDisabled={(page.data?.page ?? 1) <= 1}
                onPress={() =>
                  setQuery({
                    ...query,
                    page: { ...query.page, page: Math.max(1, query.page.page - 1) },
                  })
                }
              >
                上一页
              </Button>
              <span>{page.data?.page ?? 1} / {page.data?.total_pages ?? 1}</span>
              <Button
                size="sm"
                variant="outline"
                isDisabled={(page.data?.page ?? 1) >= (page.data?.total_pages ?? 1)}
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
            {page.isLoading && <Spinner size="sm" />}
          </Table.Footer>
        </Table>
      </div>

      <aside
        ref={detailPanelRef}
        className={[
          "min-w-0 overflow-auto border-l border-[var(--telemetry-line)] p-4 max-[1280px]:border-l-0 max-[1280px]:border-t",
          selected ? "" : "max-[1280px]:hidden",
        ].join(" ")}
      >
        {selected && (
          <Button
            className="mb-3 ml-auto"
            size="sm"
            variant="ghost"
            onPress={() => {
              setSelectedEventId(undefined);
              detail.invalidate();
            }}
          >
            关闭详情并释放报文
          </Button>
        )}
        <Tabs defaultSelectedKey="overview">
          <Tabs.ListContainer>
            <Tabs.List aria-label="抓包详情">
              <Tabs.Tab id="overview">
                概览
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="request">
                请求
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="response">
                响应
                <Tabs.Indicator />
              </Tabs.Tab>
            </Tabs.List>
          </Tabs.ListContainer>
          {selected && detail.error && (
            <Alert status="danger" className="mt-4">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>抓包详情读取失败</Alert.Title>
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
          <Tabs.Panel id="overview" className="space-y-5 pt-4">
            {!selected ? (
              <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
                选择一条抓包记录查看详情
              </p>
            ) : detail.isLoading ? (
              <div className="grid min-h-40 place-items-center">
                <Spinner aria-label="正在读取抓包详情" />
              </div>
            ) : (
              <>
                <dl className="grid grid-cols-[110px_1fr] gap-y-3 text-sm">
                  <dt>请求 ID</dt>
                  <dd className="break-all font-mono text-xs">
                    {detail.data?.request_id ?? "正在读取…"}
                  </dd>
                  <dt>终端 IP</dt>
                  <dd>{selected.terminal_ip}</dd>
                  <dt>证书指纹</dt>
                  <dd>{detail.data?.certificate_fingerprint_suffix ?? "—"}</dd>
                  <dt>TLS 状态</dt>
                  <dd>{detail.data?.tls_summary ?? "—"}</dd>
                  <dt>最终处理</dt>
                  <dd>
                    <Chip color={toneColor(selected.ui_tone)} size="sm">
                      {selected.result}
                    </Chip>
                  </dd>
                </dl>
                <div>
                  <h2 className="mb-2 font-semibold">规则执行轨迹</h2>
                  <ol className="space-y-2 text-sm">
                    {(detail.data?.rule_trace ?? []).map((entry, index) => (
                      <li key={`${entry}-${index}`} className="flex gap-2">
                        <Circle
                          aria-hidden="true"
                          className="mt-1 size-2.5 fill-green-600 text-green-600"
                        />
                        {entry}
                      </li>
                    ))}
                  </ol>
                </div>
                <Button
                  variant="outline"
                  fullWidth
                  isDisabled={!detail.data?.request_id}
                  onPress={async () => {
                    if (!detail.data?.request_id) return;
                    try {
                      await navigator.clipboard.writeText(detail.data.request_id);
                      toast("请求 ID 已复制。", { variant: "success" });
                    } catch (reason) {
                      toast(errorMessage(reason), { variant: "danger" });
                    }
                  }}
                >
                  <Copy className="size-4" />
                  复制请求 ID
                </Button>
                <Button
                  variant="outline"
                  fullWidth
                  isDisabled={!selected.can_go_to_breakpoint}
                  onPress={() => {
                    if (selected.breakpoint_id) {
                      navigate(
                        `/breakpoints?breakpointId=${encodeURIComponent(
                          selected.breakpoint_id,
                        )}`,
                      );
                    }
                  }}
                >
                  转到断点
                </Button>
                <Button
                  variant="outline"
                  fullWidth
                  onPress={createRuleFromSession}
                >
                  基于此会话新建规则
                </Button>
              </>
            )}
          </Tabs.Panel>
          <Tabs.Panel id="request" className="pt-4">
            <pre className="whitespace-pre-wrap break-all text-xs">
              {!selected
                ? "选择一条抓包记录查看请求"
                : detail.data?.request.body_text ?? "无请求正文"}
            </pre>
          </Tabs.Panel>
          <Tabs.Panel id="response" className="pt-4">
            <pre className="whitespace-pre-wrap break-all text-xs">
              {!selected
                ? "选择一条抓包记录查看响应"
                : detail.data?.response?.body_text ?? "无响应正文"}
            </pre>
          </Tabs.Panel>
        </Tabs>
      </aside>
    </section>
  );
}
