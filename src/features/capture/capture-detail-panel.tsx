import type { RefObject } from "react";
import {
  Alert,
  Button,
  Chip,
  Spinner,
  Table,
  Tabs,
  toast,
} from "@heroui/react";
import { Circle, Copy } from "@gravity-ui/icons";
import type {
  CaptureDetailViewModel,
  CaptureRowViewModel,
} from "@/generated/rust-types";
import { formatMessageBody } from "@/lib/message-content";
import { toneColor } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/client";
import { captureDetailTabLabels } from "./capture-view";

interface DetailQuery {
  data?: CaptureDetailViewModel;
  error?: string;
  isLoading: boolean;
  refresh: () => Promise<void>;
  invalidate: () => void;
}

interface CaptureDetailPanelProps {
  panelRef: RefObject<HTMLElement | null>;
  selected?: CaptureRowViewModel;
  detail: DetailQuery;
  requestHeaderCount: number;
  responseHeaderCount: number;
  onClose: () => void;
  onNavigate: (path: string) => void;
  onCreateRule: () => void;
}

function HeadersTable({
  label,
  headers,
}: {
  label: string;
  headers: Record<string, string[]>;
}) {
  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content aria-label={label}>
          <Table.Header>
            <Table.Column isRowHeader>名称</Table.Column>
            <Table.Column>值</Table.Column>
          </Table.Header>
          <Table.Body
            renderEmptyState={() => (
              <div className="p-4 text-center text-sm text-[var(--telemetry-muted)]">
                无 HTTP Header
              </div>
            )}
          >
            {Object.entries(headers).flatMap(([name, values]) =>
              values.map((value, index) => (
                <Table.Row key={`${name}-${index}`}>
                  <Table.Cell className="font-mono text-xs">{name}</Table.Cell>
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
  );
}

export function CaptureDetailPanel({
  panelRef,
  selected,
  detail,
  requestHeaderCount,
  responseHeaderCount,
  onClose,
  onNavigate,
  onCreateRule,
}: CaptureDetailPanelProps) {
  return (
    <aside
      ref={panelRef}
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
          onPress={onClose}
        >
          关闭详情并释放报文
        </Button>
      )}
      <Tabs defaultSelectedKey="overview">
        <Tabs.ListContainer>
          <Tabs.List aria-label="抓包详情">
            {Object.entries(captureDetailTabLabels).map(([id, label]) => (
              <Tabs.Tab key={id} id={id} className="whitespace-nowrap">
                {label}
                <Tabs.Indicator />
              </Tabs.Tab>
            ))}
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
              <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
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
                <dt>HTTP 状态码</dt>
                <dd>
                  <Chip size="sm" color="accent" variant="soft">
                    {detail.data?.response?.http_status ?? "等待响应"}
                  </Chip>
                </dd>
                <dt>请求 Header</dt>
                <dd>{requestHeaderCount} 项</dd>
                <dt>响应 Header</dt>
                <dd>
                  {detail.data?.response
                    ? `${responseHeaderCount} 项`
                    : "等待响应"}
                </dd>
              </dl>
              {Object.keys(detail.data?.extracted_metadata ?? {}).length >
                0 && (
                <div>
                  <h2 className="mb-2 font-semibold">Workspace 提取结果</h2>
                  <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
                    {Object.entries(detail.data?.extracted_metadata ?? {}).map(
                      ([name, value]) => (
                        <div key={name} className="contents">
                          <dt>{name}</dt>
                          <dd className="break-all font-mono text-xs">
                            {value}
                          </dd>
                        </div>
                      ),
                    )}
                  </dl>
                </div>
              )}
              {(detail.data?.response_assertions.length ?? 0) > 0 && (
                <div>
                  <h2 className="mb-2 font-semibold">响应断言</h2>
                  <ul className="space-y-2 text-sm">
                    {(detail.data?.response_assertions ?? []).map(
                      (assertion) => (
                        <li
                          key={assertion.assertion_id}
                          className="flex items-start gap-2"
                        >
                          <Chip
                            size="sm"
                            color={assertion.passed ? "success" : "danger"}
                            variant="soft"
                          >
                            {assertion.passed ? "通过" : "失败"}
                          </Chip>
                          <span>
                            <strong>{assertion.name}</strong>
                            <br />
                            <span className="text-xs text-[var(--telemetry-muted)]">
                              {assertion.message}
                            </span>
                          </span>
                        </li>
                      ),
                    )}
                  </ul>
                </div>
              )}
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
                onPress={() =>
                  selected.breakpoint_id &&
                  onNavigate(
                    `/breakpoints?breakpointId=${encodeURIComponent(selected.breakpoint_id)}`,
                  )
                }
              >
                转到断点
              </Button>
              <Button variant="outline" fullWidth onPress={onCreateRule}>
                基于此会话新建规则
              </Button>
            </>
          )}
        </Tabs.Panel>
        <Tabs.Panel id="request" className="space-y-4 pt-4">
          {!selected ? (
            <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
              选择一条抓包记录查看请求
            </p>
          ) : (
            <>
              <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
                <dt>请求行</dt>
                <dd className="break-all font-mono text-xs">
                  {selected.method} {selected.target}
                </dd>
                <dt>Header 数量</dt>
                <dd>{requestHeaderCount}</dd>
              </dl>
              <div>
                <h2 className="mb-2 font-semibold">请求 Header</h2>
                <HeadersTable
                  label="请求 HTTP Header"
                  headers={detail.data?.request.headers ?? {}}
                />
              </div>
              <div>
                <h2 className="mb-2 font-semibold">请求 Body</h2>
                <pre className="max-h-80 overflow-auto whitespace-pre font-mono text-xs">
                  {formatMessageBody(detail.data?.request, "无请求正文")}
                </pre>
              </div>
            </>
          )}
        </Tabs.Panel>
        <Tabs.Panel id="response" className="space-y-4 pt-4">
          {!selected ? (
            <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
              选择一条抓包记录查看响应
            </p>
          ) : !detail.data?.response ? (
            <p className="py-12 text-center text-sm text-[var(--telemetry-muted)]">
              当前会话没有响应报文
            </p>
          ) : (
            <>
              <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-3 text-sm">
                <dt>HTTP 状态码</dt>
                <dd>
                  <Chip size="sm" color="accent" variant="soft">
                    {detail.data.response.http_status ?? "未知"}
                  </Chip>
                </dd>
                <dt>Header 数量</dt>
                <dd>{responseHeaderCount}</dd>
              </dl>
              <div>
                <h2 className="mb-2 font-semibold">响应 Header</h2>
                <HeadersTable
                  label="响应 HTTP Header"
                  headers={detail.data.response.headers}
                />
              </div>
              <div>
                <h2 className="mb-2 font-semibold">响应 Body</h2>
                <pre className="max-h-80 overflow-auto whitespace-pre font-mono text-xs">
                  {formatMessageBody(detail.data.response, "无响应正文")}
                </pre>
              </div>
            </>
          )}
        </Tabs.Panel>
      </Tabs>
    </aside>
  );
}
