"use client";

/**
 * 跨桌面、ADB、设备端 VPN/TUN 和代理链路的统一诊断日志页。
 *
 * 页面只查询、筛选和显示 Rust 生成的有界控制面 ViewModel。控制通道与业务数据通道
 * 必须分开记录：adb forward 只承载桌面到 Companion 的控制命令，adb reverse
 * 承载设备到桌面代理入口的业务连接。
 */

import {
  Alert,
  Button,
  Card,
  Chip,
  Input,
  ListBox,
  Select,
  Spinner,
  Table,
  toast,
} from "@heroui/react";
import { useMemo, useState } from "react";
import { useAppEventRefresh } from "@/features/shell/bootstrap-context";
import { commands } from "@/generated/rust-types";
import type {
  DiagnosticLogPageViewModel,
  DiagnosticLogQuery,
  DiagnosticLogRowViewModel,
  ProxyWorkspace,
  WorkspaceSummaryViewModel,
} from "@/generated/rust-types";
import { formatTimestamp, toneColor } from "@/lib/format";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import { useIpcQuery } from "@/lib/ipc/use-ipc-query";

const refreshEvents = [
  "diagnostic_log_added",
  "listener_status_changed",
  "android_vpn_status_changed",
  "session_updated",
  "resource_warning",
  "operation_failed",
  "snapshot_required",
] as const;

function contextText(row: DiagnosticLogRowViewModel): string {
  return [
    row.device_serial ? `设备 ${row.device_serial}` : null,
    row.listener_id ? `入口 ${row.listener_id}` : null,
    row.profile_id ? `方案 ${row.profile_id}` : null,
  ]
    .filter(Boolean)
    .join(" · ") || "—";
}

export function DiagnosticLogsView() {
  const [draftKeyword, setDraftKeyword] = useState("");
  const [keyword, setKeyword] = useState("");
  const [reportListenerId, setReportListenerId] = useState<string>();
  const [reportPending, setReportPending] = useState(false);
  const query = useMemo<DiagnosticLogQuery>(
    () => ({ keyword: keyword || null, after_event_id: null, limit: 300 }),
    [keyword],
  );
  const queryKey = useMemo(() => JSON.stringify(query), [query]);
  const page = useIpcQuery<DiagnosticLogPageViewModel>(
    `diagnostic-log-query:${queryKey}`,
    () => callCommand(commands.diagnosticLogQuery(query)),
  );
  const workspaces = useIpcQuery<WorkspaceSummaryViewModel[]>(
    "diagnostic-report-workspaces",
    () => callCommand(commands.workspaceList()),
  );
  const workspaceId = workspaces.data?.find((item) => item.selected)?.id;
  const workspace = useIpcQuery<ProxyWorkspace>(
    `diagnostic-report-workspace:${workspaceId ?? "none"}`,
    () => callCommand(commands.workspaceGet(workspaceId!)),
    undefined,
    { enabled: Boolean(workspaceId) },
  );
  const effectiveReportListenerId = workspace.data?.listeners.some(
    (listener) => listener.id === reportListenerId,
  )
    ? reportListenerId
    : workspace.data?.listeners[0]?.id;
  useAppEventRefresh(refreshEvents, page.refresh);
  useAppEventRefresh(
    ["workspace_changed", "listener_status_changed", "snapshot_required"],
    async () => {
      await Promise.all([workspaces.refresh(), workspace.refresh()]);
    },
  );

  const exportReport = async () => {
    if (!workspaceId || !effectiveReportListenerId) return;
    setReportPending(true);
    try {
      const result = await callCommand(commands.diagnosticReproductionReportExport({
        workspace_id: workspaceId,
        listener_id: effectiveReportListenerId,
      }));
      if (result) {
        toast(`故障复现报告已导出（${result.bytes_written} 字节）。`, {
          variant: "success",
        });
      }
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setReportPending(false);
    }
  };

  return (
    <section className="space-y-5 p-5">
      <div>
        <h1 className="sr-only">诊断日志</h1>
        <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
          汇总已记录的 ADB、设备网络、代理入口、TLS 与 HTTP 诊断事件，便于定位失败阶段。
        </p>
      </div>

      <Alert status="accent">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>控制通道与业务通道相互独立</Alert.Title>
          <Alert.Description>
            ADB forward 用于桌面控制 Companion；设备与桌面同网段且入口允许 LAN 时，业务连接会直连桌面，否则回退到 ADB reverse。部分 OEM 的 ADB reverse
            可能只接受设备侧连接却未转发到主机，此时应使用同网段 LAN 或独立 USB 数据隧道。
          </Alert.Description>
        </Alert.Content>
      </Alert>

      {page.data?.snapshot_required && (
        <Alert status="warning">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>诊断事件游标已过期</Alert.Title>
            <Alert.Description>
              旧事件已超出 EventHub 的条数或字节保留窗口；当前列表是重新读取的完整保留快照。
            </Alert.Description>
          </Alert.Content>
        </Alert>
      )}

      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Header>
          <Card.Title>故障复现报告</Card.Title>
          <Card.Description>
            聚合当前 Workspace、精确入口、运行状态、转发方式、规则、协议包、Android 网络、最近抓包、结构化诊断与持久化应用日志，导出为可复制的 Markdown。
          </Card.Description>
        </Card.Header>
        <Card.Content className="grid items-end gap-3 px-4 pb-4 md:grid-cols-[minmax(0,1fr)_auto]">
          <Select
            aria-label="选择复现报告入口"
            isDisabled={reportPending || workspace.isLoading || !workspace.data?.listeners.length}
            selectedKey={effectiveReportListenerId}
            onSelectionChange={(key) => setReportListenerId(String(key))}
          >
            <Select.Trigger className="h-11 min-h-11 min-w-0">
              <Select.Value className="min-w-0 flex-1 truncate">
                {({ selectedText }) => selectedText || "选择入口"}
              </Select.Value>
              <Select.Indicator className="shrink-0" />
            </Select.Trigger>
            <Select.Popover>
              <ListBox>
                {(workspace.data?.listeners ?? []).map((listener) => (
                  <ListBox.Item id={listener.id} key={listener.id} textValue={listener.name}>
                    <span>{listener.name}</span>
                    <span className="ml-2 text-xs text-[var(--telemetry-muted)]">
                      {listener.bind_address}:{listener.port}
                    </span>
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
          <Button
            variant="primary"
            isPending={reportPending}
            isDisabled={!workspaceId || !effectiveReportListenerId || Boolean(workspace.error)}
            onPress={() => void exportReport()}
          >
            导出复现 Markdown
          </Button>
          {(workspaces.error || workspace.error) && (
            <p className="text-sm text-[var(--telemetry-danger)] md:col-span-2">
              无法读取报告范围：{workspaces.error ?? workspace.error}
            </p>
          )}
        </Card.Content>
      </Card>

      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Header>
          <Card.Title>运行诊断记录</Card.Title>
          <Card.Description>
            本表显示有界的结构化诊断事件；完整进程运行日志另行持久化并可由 MCP 分页查询。某阶段没有记录不代表该阶段成功。
          </Card.Description>
        </Card.Header>
        <Card.Content className="space-y-4 p-0">
          <div className="flex gap-2 px-4 max-[700px]:flex-col">
            <Input
              aria-label="筛选诊断日志"
              placeholder="搜索阶段、摘要、设备序列号、入口或方案 ID"
              value={draftKeyword}
              onChange={(event) => setDraftKeyword(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") setKeyword(draftKeyword.trim());
              }}
            />
            <Button variant="primary" onPress={() => setKeyword(draftKeyword.trim())}>
              筛选
            </Button>
            <Button variant="outline" onPress={() => void page.refresh()}>
              刷新
            </Button>
          </div>

          {page.error && (
            <Alert status="danger" className="mx-4">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>诊断日志读取失败</Alert.Title>
                <Alert.Description>{page.error}</Alert.Description>
              </Alert.Content>
              <Button size="sm" variant="outline" onPress={() => void page.refresh()}>
                重试
              </Button>
            </Alert>
          )}

          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="诊断日志" className="min-w-[1080px]">
                <Table.Header>
                  <Table.Column isRowHeader>时间</Table.Column>
                  <Table.Column>级别</Table.Column>
                  <Table.Column>阶段</Table.Column>
                  <Table.Column>摘要与详情</Table.Column>
                  <Table.Column>关联对象</Table.Column>
                </Table.Header>
                <Table.Body
                  renderEmptyState={() => (
                    <div className="p-10 text-center text-sm text-[var(--telemetry-muted)]">
                      {page.isLoading ? (
                        <Spinner size="sm" aria-label="正在读取诊断日志" />
                      ) : (
                        page.data?.empty_message ?? "暂无诊断日志"
                      )}
                    </div>
                  )}
                >
                  {(page.data?.rows ?? []).map((row) => (
                    <Table.Row key={row.event_id} id={row.event_id}>
                      <Table.Cell className="whitespace-nowrap font-mono text-xs">
                        {formatTimestamp(row.occurred_at)}
                      </Table.Cell>
                      <Table.Cell>
                        <Chip size="sm" color={toneColor(row.ui_tone)} variant="soft">
                          {row.level_text}
                        </Chip>
                      </Table.Cell>
                      <Table.Cell>
                        <Chip size="sm" variant="secondary">{row.stage_text}</Chip>
                      </Table.Cell>
                      <Table.Cell>
                        <div className="font-medium">{row.summary}</div>
                        {row.detail && (
                          <div className="mt-1 break-all text-xs text-[var(--telemetry-muted)]">
                            {row.detail}
                          </div>
                        )}
                      </Table.Cell>
                      <Table.Cell className="max-w-72 break-all text-xs">
                        {contextText(row)}
                      </Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>

          <div className="flex justify-between px-4 pb-4 text-xs text-[var(--telemetry-muted)]">
            <span>内存保留 {page.data?.retained_count ?? 0} 条</span>
            {page.data?.truncated && <span>已达到查询上限，仅显示最新记录</span>}
          </div>
        </Card.Content>
      </Card>
    </section>
  );
}
