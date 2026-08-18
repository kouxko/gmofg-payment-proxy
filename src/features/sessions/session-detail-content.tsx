import { Alert, Button, Chip, Spinner, Table, Tabs } from "@heroui/react";
import type {
  SessionDetailViewModel,
  SessionSummaryViewModel,
} from "@/generated/rust-types";
import type { QueryAwareRequest } from "@/lib/message-content";
import {
  HttpBodyViewer,
  HttpRequestTargetView,
} from "@/features/shared/http-inspection";
import { sessionDetailTabLabels } from "./session-config";

interface DetailQuery {
  data?: SessionDetailViewModel;
  error?: string;
  isLoading: boolean;
  refresh: () => Promise<void>;
}
interface SessionDetailContentProps {
  selected?: SessionSummaryViewModel;
  detail: DetailQuery;
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

export function SessionDetailContent({
  selected,
  detail,
}: SessionDetailContentProps) {
  return (
    <>
      {selected && detail.error && (
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
            {Object.entries(sessionDetailTabLabels).map(([id, label]) => (
              <Tabs.Tab key={id} id={id}>
                {label}
                <Tabs.Indicator />
              </Tabs.Tab>
            ))}
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
                <dd className="break-all font-mono text-xs">
                  {selected.request_id}
                </dd>
                <dt>终端证书指纹</dt>
                <dd className="break-all">
                  {detail.data?.certificate_fingerprint ?? "正在读取…"}
                </dd>
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
        <Tabs.Panel id="request" className="space-y-4 pt-4">
          {selected && (
            <HttpRequestTargetView
              method={selected.method}
              target={selected.target}
              queryString={(selected as SessionSummaryViewModel & QueryAwareRequest).query_string}
            />
          )}
          <h2 className="font-semibold">请求 Header</h2>
          <HeadersTable
            label="详情请求 HTTP Header"
            headers={detail.data?.request?.headers ?? {}}
          />
          <h2 className="font-semibold">请求 Body</h2>
          <HttpBodyViewer
            label="请求 Body"
            message={detail.data?.request}
            emptyText="按需读取后显示请求报文"
          />
        </Tabs.Panel>
        <Tabs.Panel id="response" className="space-y-4 pt-4">
          <h2 className="font-semibold">响应 Header</h2>
          <HeadersTable
            label="详情响应 HTTP Header"
            headers={detail.data?.response?.headers ?? {}}
          />
          <h2 className="font-semibold">响应 Body</h2>
          <HttpBodyViewer
            label="响应 Body"
            message={detail.data?.response}
            emptyText="按需读取后显示响应报文"
          />
        </Tabs.Panel>
      </Tabs>
    </>
  );
}
