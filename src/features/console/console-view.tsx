"use client";

/**
 * 代理控制台的展示与用户操作组件。
 *
 * status 和 recentCapture 都是 Rust 已经整理好的 ViewModel。组件只决定如何用
 * HeroUI 展示、何时调用启动/停止/重启命令，以及如何反馈执行结果；端口占用、
 * 证书就绪、状态迁移和资源清理全部由 Rust 判断。
 */

import { useState } from "react";
import {
  Alert,
  Button,
  Card,
  Chip,
  ProgressBar,
  Table,
  toast,
} from "@heroui/react";
import {
  ArrowRotateRight,
  CirclePlayFill,
  CircleStopFill,
} from "@gravity-ui/icons";
import type {
  CapturePageViewModel,
  ProxyStatusViewModel,
} from "@/generated/rust-types";
import { commands } from "@/generated/rust-types";
import { callCommand, errorMessage } from "@/lib/ipc/client";
import {
  formatDuration,
  formatTimestamp,
  toneColor,
} from "@/lib/format";

export function ConsoleView({
  status,
  recentCapture,
  recentCaptureError,
  recentCaptureLoading,
  onRecentCaptureRetry,
  onRefresh,
}: {
  status: ProxyStatusViewModel;
  recentCapture?: CapturePageViewModel;
  recentCaptureError?: string;
  recentCaptureLoading: boolean;
  onRecentCaptureRetry: () => Promise<void>;
  onRefresh: () => Promise<void>;
}) {
  const [pendingOperation, setPendingOperation] = useState<
    "start" | "stop" | "restart"
  >();
  const connectionHealth = [
    { label: "App → Proxy", health: status.app_to_proxy_health },
    { label: "Proxy → Server", health: status.proxy_to_server_health },
  ];

  async function run(
    operation: "start" | "stop" | "restart",
    command: () => ReturnType<
      | typeof commands.proxyStart
      | typeof commands.proxyStop
      | typeof commands.proxyRestart
    >,
  ) {
    // pendingOperation 既控制按钮文案，也阻止用户重复提交同一个生命周期操作。
    if (pendingOperation) return;
    setPendingOperation(operation);
    try {
      const result = await callCommand(command());
      // 命令成功后重新取得全局快照，确保表格显示的是 Rust 的最终状态。
      toast(result.state_text, { variant: toneColor(result.ui_tone) });
      await onRefresh();
    } catch (reason) {
      toast(errorMessage(reason), { variant: "danger" });
    } finally {
      setPendingOperation(undefined);
    }
  }

  return (
    <section className="space-y-4 p-5">
      <div className="flex items-center">
        <div>
          <h1 className="text-2xl font-semibold">代理控制台</h1>
          <p className="mt-1 text-sm text-[var(--telemetry-muted)]">
            双向 mTLS 代理的运行状态、连接健康与最近事件
          </p>
        </div>
        <div className="ml-auto flex gap-3">
          {status.can_start && (
            <Button
              variant="primary"
              isDisabled={pendingOperation != null}
              onPress={() => void run("start", commands.proxyStart)}
            >
              <CirclePlayFill className="size-4" />
              {pendingOperation === "start" ? "正在启动…" : "启动代理"}
            </Button>
          )}
          <Button
            variant="outline"
            isDisabled={!status.can_stop || pendingOperation != null}
            onPress={() => void run("stop", commands.proxyStop)}
          >
            <CircleStopFill className="size-4" />
            {pendingOperation === "stop" ? "正在停止…" : "停止代理"}
          </Button>
          <Button
            variant="outline"
            isDisabled={!status.can_restart || pendingOperation != null}
            onPress={() => void run("restart", commands.proxyRestart)}
          >
            <ArrowRotateRight className="size-4" />
            {pendingOperation === "restart" ? "正在重启…" : "重启代理"}
          </Button>
        </div>
      </div>

      <Alert
        status={
          status.ui_tone === "danger"
            ? "danger"
            : status.ui_tone === "warning"
              ? "warning"
              : status.ui_tone === "positive"
                ? "success"
                : "accent"
        }
      >
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>代理{status.state_text}</Alert.Title>
          <Alert.Description>
            {status.fault_reason ??
              "所有产品通道均由 Rust 统一管理，前端仅显示当前运行快照。"}
          </Alert.Description>
        </Alert.Content>
      </Alert>

      <Table>
        <Table.ScrollContainer>
          <Table.Content
            aria-label="代理通道状态"
            className="min-w-[1080px]"
          >
            <Table.Header>
              <Table.Column isRowHeader>通道</Table.Column>
              <Table.Column>监听地址</Table.Column>
              <Table.Column>状态</Table.Column>
              <Table.Column>上游地址</Table.Column>
              <Table.Column>上游状态</Table.Column>
              <Table.Column>已连接终端</Table.Column>
              <Table.Column>请求数</Table.Column>
              <Table.Column>错误数</Table.Column>
              <Table.Column>启用</Table.Column>
            </Table.Header>
            <Table.Body>
              {status.channels.map((channel) => (
                <Table.Row key={channel.id} id={channel.id}>
                  <Table.Cell>
                    <div className="font-medium">{channel.display_name}</div>
                    <div className="text-xs text-[var(--telemetry-muted)]">
                      HTTPS / {channel.mtls_enabled ? "mTLS" : "TLS"}
                    </div>
                  </Table.Cell>
                  <Table.Cell className="font-mono text-xs">
                    {channel.listen_address}
                  </Table.Cell>
                  <Table.Cell>
                    <Chip
                      size="sm"
                      color={toneColor(channel.ui_tone)}
                      variant="soft"
                    >
                      {channel.state_text}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell className="max-w-64 break-all font-mono text-xs">
                    {channel.upstream_url}
                  </Table.Cell>
                  <Table.Cell>
                    <Chip
                      size="sm"
                      color={toneColor(channel.upstream_ui_tone)}
                      variant="soft"
                    >
                      {channel.upstream_state_text}
                    </Chip>
                  </Table.Cell>
                  <Table.Cell>{channel.connected_clients}</Table.Cell>
                  <Table.Cell>{channel.request_count}</Table.Cell>
                  <Table.Cell>{channel.error_count}</Table.Cell>
                  <Table.Cell>{channel.enabled ? "已启用" : "已禁用"}</Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
      </Table>

      <div className="grid grid-cols-[minmax(0,1fr)_360px] gap-4 max-[1180px]:grid-cols-1">
        <Card>
          <Card.Header>
            <Card.Title>连接路径与最近事件</Card.Title>
            <Card.Description>
              App → Proxy 与 Proxy → Server 分别报告，不合并状态
            </Card.Description>
          </Card.Header>
          <Card.Content className="space-y-4">
            {recentCaptureError && (
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>最近事件读取失败</Alert.Title>
                  <Alert.Description>{recentCaptureError}</Alert.Description>
                </Alert.Content>
                <Button
                  size="sm"
                  variant="outline"
                  onPress={() => void onRecentCaptureRetry()}
                >
                  重试
                </Button>
              </Alert>
            )}
            <div className="grid grid-cols-2 gap-3">
              {connectionHealth.map(({ label, health }) => (
                <Alert
                  key={label}
                  status={
                    health.ui_tone === "danger"
                      ? "danger"
                      : health.ui_tone === "warning"
                        ? "warning"
                        : health.ui_tone === "positive"
                          ? "success"
                          : "accent"
                  }
                >
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>{label}</Alert.Title>
                    <Alert.Description>
                      {health.state_text} · {health.detail}
                    </Alert.Description>
                  </Alert.Content>
                </Alert>
              ))}
            </div>
            <Table>
              <Table.ScrollContainer>
                <Table.Content
                  aria-label="最近事件"
                  className="min-w-[760px]"
                >
                  <Table.Header>
                    <Table.Column isRowHeader>时间</Table.Column>
                    <Table.Column>终端</Table.Column>
                    <Table.Column>阶段</Table.Column>
                    <Table.Column>事件</Table.Column>
                    <Table.Column>耗时</Table.Column>
                  </Table.Header>
                  <Table.Body
                    renderEmptyState={() => (
                      <div className="p-6 text-center text-sm">
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
                        <Table.Cell>
                          {formatDuration(row.duration_ms)}
                        </Table.Cell>
                      </Table.Row>
                    ))}
                  </Table.Body>
                </Table.Content>
              </Table.ScrollContainer>
            </Table>
          </Card.Content>
        </Card>

        <Card>
          <Card.Header>
            <Card.Title>运行信息</Card.Title>
          </Card.Header>
          <Card.Content className="space-y-4 text-sm">
            <dl className="grid grid-cols-2 gap-x-4 gap-y-3">
              <dt className="text-[var(--telemetry-muted)]">活动会话</dt>
              <dd className="text-right">{status.active_sessions}</dd>
              <dt className="text-[var(--telemetry-muted)]">待处理断点</dt>
              <dd className="text-right">{status.pending_breakpoints}</dd>
              <dt className="text-[var(--telemetry-muted)]">内存使用</dt>
              <dd className="text-right">{status.logical_memory_text}</dd>
              <dt className="text-[var(--telemetry-muted)]">会话容量</dt>
              <dd className="text-right">{status.session_capacity}</dd>
              <dt className="text-[var(--telemetry-muted)]">默认超时</dt>
              <dd className="text-right">{status.default_timeout_seconds} 秒</dd>
            </dl>
            <ProgressBar
              aria-label="内存容量"
              value={status.memory_usage_percent}
            >
              <ProgressBar.Output />
              <ProgressBar.Track>
                <ProgressBar.Fill />
              </ProgressBar.Track>
            </ProgressBar>
          </Card.Content>
        </Card>
      </div>
    </section>
  );
}
