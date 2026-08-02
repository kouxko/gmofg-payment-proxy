"use client";

/**
 * 运行监控只展示 Rust 合并后的 Workspace 入口概览和最近事件。
 *
 * 入口的新增、地址、上游和启停统一在“入口配置”中操作；本页不再调用旧的全局
 * ProxySupervisor，避免出现第二套通道目录和生命周期按钮。
 */

import { Alert, Button, Card, Chip, Table } from "@heroui/react";
import { Server } from "@gravity-ui/icons";
import type {
  CapturePageViewModel,
  ListenerOverviewViewModel,
} from "@/generated/rust-types";
import { formatDuration, formatTimestamp, toneColor } from "@/lib/format";
import { useWorkspaceNavigation } from "@/features/shell/workspace-navigation";

function alertStatus(tone: ListenerOverviewViewModel["ui_tone"]) {
  if (tone === "danger") return "danger" as const;
  if (tone === "warning") return "warning" as const;
  if (tone === "positive") return "success" as const;
  return "accent" as const;
}

export function ConsoleView({
  overview,
  recentCapture,
  recentCaptureError,
  recentCaptureLoading,
  onRecentCaptureRetry,
}: {
  overview: ListenerOverviewViewModel;
  recentCapture?: CapturePageViewModel;
  recentCaptureError?: string;
  recentCaptureLoading: boolean;
  onRecentCaptureRetry: () => Promise<void>;
}) {
  const { navigate } = useWorkspaceNavigation();

  return (
    <section className="space-y-4 p-5">
      <div className="flex flex-wrap items-center gap-3">
        <div>
          <h1 className="text-2xl font-semibold">运行监控</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            当前工作区的代理入口状态与最近流量事件
          </p>
        </div>
        <Button
          className="ml-auto"
          variant="primary"
          onPress={() => navigate("/listeners")}
        >
          <Server className="size-4" />
          管理代理入口
        </Button>
      </div>

      <Alert status={alertStatus(overview.ui_tone)}>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{overview.state_text}</Alert.Title>
          <Alert.Description>
            {overview.workspace_name}：共 {overview.total_count} 个入口，当前活动 {overview.active_count} 个
            {overview.faulted_count > 0 ? `，故障 ${overview.faulted_count} 个` : ""}。入口需要在“入口配置”中逐个校验、保存和启停。
          </Alert.Description>
        </Alert.Content>
      </Alert>

      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Header>
          <Card.Title>代理入口运行状态</Card.Title>
          <Card.Description>配置与运行状态均由 Rust 汇总</Card.Description>
        </Card.Header>
        <Card.Content className="p-0">
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="代理入口运行状态" className="min-w-[820px]">
                <Table.Header>
                  <Table.Column isRowHeader>入口</Table.Column>
                  <Table.Column>监听地址</Table.Column>
                  <Table.Column>请求去向</Table.Column>
                  <Table.Column>状态</Table.Column>
                  <Table.Column>故障原因</Table.Column>
                </Table.Header>
                <Table.Body
                  renderEmptyState={() => (
                    <div className="p-8 text-center text-sm text-[var(--telemetry-muted)]">
                      当前工作区还没有代理入口，请先进入“入口配置”创建。
                    </div>
                  )}
                >
                  {overview.rows.map((row) => (
                    <Table.Row key={row.listener_id} id={row.listener_id}>
                      <Table.Cell>
                        <div className="font-medium">{row.name}</div>
                        <div className="text-xs text-[var(--telemetry-muted)]">{row.kind_text}</div>
                      </Table.Cell>
                      <Table.Cell className="font-mono text-xs">{row.listen_address}</Table.Cell>
                      <Table.Cell className="max-w-80 break-all font-mono text-xs">
                        {row.request_destination}
                      </Table.Cell>
                      <Table.Cell>
                        <Chip size="sm" color={toneColor(row.ui_tone)} variant="soft">
                          {row.state_text}
                        </Chip>
                      </Table.Cell>
                      <Table.Cell>{row.fault_reason ?? "—"}</Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </Card.Content>
      </Card>

      <Card className="border border-[var(--telemetry-line)] shadow-sm">
        <Card.Header>
          <Card.Title>最近流量事件</Card.Title>
          <Card.Description>入口收到请求后，抓包、规则和故障结果会出现在这里</Card.Description>
        </Card.Header>
        <Card.Content className="space-y-3 p-0">
          {recentCaptureError && (
            <Alert status="danger" className="mx-4 mt-4">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>最近事件读取失败</Alert.Title>
                <Alert.Description>{recentCaptureError}</Alert.Description>
              </Alert.Content>
              <Button size="sm" variant="outline" onPress={() => void onRecentCaptureRetry()}>
                重试
              </Button>
            </Alert>
          )}
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="最近事件" className="min-w-[760px]">
                <Table.Header>
                  <Table.Column isRowHeader>时间</Table.Column>
                  <Table.Column>终端</Table.Column>
                  <Table.Column>阶段</Table.Column>
                  <Table.Column>结果</Table.Column>
                  <Table.Column>耗时</Table.Column>
                </Table.Header>
                <Table.Body
                  renderEmptyState={() => (
                    <div className="p-8 text-center text-sm text-[var(--telemetry-muted)]">
                      {recentCaptureLoading
                        ? "正在读取最近事件…"
                        : recentCaptureError
                          ? "最近事件暂不可用"
                          : recentCapture?.empty_message ?? "暂无最近事件"}
                    </div>
                  )}
                >
                  {(recentCapture?.rows ?? []).map((row) => (
                    <Table.Row key={row.event_id} id={row.event_id}>
                      <Table.Cell>{formatTimestamp(row.occurred_at)}</Table.Cell>
                      <Table.Cell>{row.terminal_ip}</Table.Cell>
                      <Table.Cell>{row.stage_text}</Table.Cell>
                      <Table.Cell>{row.result}</Table.Cell>
                      <Table.Cell>{formatDuration(row.duration_ms)}</Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </Card.Content>
      </Card>
    </section>
  );
}
