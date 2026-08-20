import { Alert, Button, Chip, Modal, Spinner } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import type {
  SocketCaptureDetailViewModel,
  SocketCaptureRowViewModel,
  SocketDisplayFallbackReason,
  SocketLocalExchangeFailureStage,
} from "@/generated/rust-types";
import { formatBytes, formatTimestamp } from "@/lib/format";
import { packageLabel, schemaLabel } from "./socket-capture-model";
import {
  PaginatedHex,
  ProtocolHexViewer,
  SocketDocumentView,
} from "./socket-byte-viewer";

interface SocketCaptureDetailProps {
  selected?: SocketCaptureRowViewModel;
  detail?: SocketCaptureDetailViewModel;
  error?: string;
  malformed: boolean;
  loading: boolean;
  onClose: () => void;
  onRetry: () => void;
}

const fallbackText: Record<SocketDisplayFallbackReason, string> = {
  entry_point_failed: "协议视图生成失败，默认显示 Hex",
  resource_limit_exceeded: "协议视图超出脚本资源限制，默认显示 Hex",
};

const failureTitle: Record<SocketLocalExchangeFailureStage, string> = {
  response_rule: "响应规则失败",
  response_encode: "响应生成失败",
  response_write: "响应写回失败",
};

function DisplayFallback({ display }: { display: Extract<import("@/generated/rust-types").SocketDisplayResult, { type: "hex_fallback" }> }) {
  return (
    <div className="text-sm text-[var(--telemetry-muted)]">
      <p>{fallbackText[display.reason]}</p>
      {display.diagnostic && <p><code>{display.diagnostic.code}</code> · {display.diagnostic.message}</p>}
    </div>
  );
}

function Rules({ ids }: { ids: string[] }) {
  return ids.length === 0 ? <span className="text-[var(--telemetry-muted)]">无规则命中</span> : (
    <ul className="space-y-1">{ids.map((id) => <li key={id}><code className="text-xs">{id}</code></li>)}</ul>
  );
}

function CommonMetadata({ row, detail }: { row: SocketCaptureRowViewModel; detail?: SocketCaptureDetailViewModel }) {
  return (
    <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
      <dt>时间</dt><dd>{formatTimestamp(row.occurred_at)}</dd>
      <dt>记录 ID</dt><dd className="break-all font-mono text-xs">{row.capture_id}</dd>
      <dt>对端地址</dt><dd className="break-all font-mono text-xs">{detail?.record.peer_address ?? "正在读取…"}</dd>
      <dt>协议包</dt><dd><code className="text-xs">{packageLabel(row.package)}</code></dd>
      <dt>字段结构</dt><dd><code className="text-xs">{schemaLabel(row.schema)}</code></dd>
      <dt>连接</dt><dd className="break-all font-mono text-xs">{row.connection_id}</dd>
      <dt>关联交换</dt><dd className="break-all font-mono text-xs">{row.session_id}</dd>
      <dt>入口</dt><dd className="break-all font-mono text-xs">{row.listener_id}</dd>
      <dt>数据量</dt><dd>原始 {formatBytes(row.origin_size_bytes)} · 写出 {formatBytes(row.written_size_bytes)} · 解析 {formatBytes(row.logical_size_bytes)}</dd>
    </dl>
  );
}

function RelayDetail({ detail }: { detail: SocketCaptureDetailViewModel }) {
  if (detail.record.payload.kind !== "relay_frame") return null;
  const frame = detail.record.payload.capture;
  return (
    <div className="space-y-6">
      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
        <dt>方向</dt><dd>{frame.direction === "upstream" ? "App → Server" : "Server → App"}</dd>
        <dt>处理链</dt><dd><Chip size="sm" color="accent" variant="soft">Decode → 两段规则 → Encode</Chip></dd>
      </dl>
      <section className="space-y-3">
        <h2 className="font-semibold">原始报文</h2>
        <PaginatedHex bytes={frame.origin} label="转发原始报文" />
      </section>
      {frame.stages.map((stage) => (
        <section className="space-y-3" key={stage.stage}>
          <h2 className="font-semibold">{stage.stage === "app_to_proxy" ? "应用 → 代理" : stage.stage === "proxy_to_upstream" ? "代理 → 上游服务" : stage.stage === "upstream_to_proxy" ? "上游服务 → 代理" : "代理 → 应用"}</h2>
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
            <dt>命中规则</dt><dd><Rules ids={stage.matched_rule_ids} /></dd>
          </dl>
          <SocketDocumentView document={stage.document} />
        </section>
      ))}
      <section className="space-y-3">
        <h2 className="font-semibold">写出报文</h2>
        {frame.display.type === "hex_fallback" && <DisplayFallback display={frame.display} />}
        <ProtocolHexViewer bytes={frame.written} display={frame.display} label="转发写出报文" />
      </section>
    </div>
  );
}

function LocalDetail({ detail }: { detail: SocketCaptureDetailViewModel }) {
  if (detail.record.payload.kind !== "local_exchange") return null;
  const exchange = detail.record.payload.capture;
  return (
    <div className="space-y-3">
      <p className="text-sm">关联交换 ID：<code>{exchange.exchange_id}</code></p>
      <div className="grid gap-6 xl:grid-cols-2">
        <section className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
          <div><h2 className="font-semibold">本机应答请求</h2><p className="text-xs text-[var(--telemetry-muted)]">应用 → 本机应答</p></div>
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
            <dt>命中规则</dt><dd><Rules ids={exchange.matched_request_rule_ids} /></dd>
          </dl>
          <ProtocolHexViewer bytes={exchange.request_origin} document={exchange.request_document} display={exchange.request_display} label="本机应答请求" />
        </section>
        <section className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
          <div><h2 className="font-semibold">本机应答响应</h2><p className="text-xs text-[var(--telemetry-muted)]">本机应答 → 应用</p></div>
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
            <dt>命中规则</dt><dd><Rules ids={exchange.matched_response_rule_ids} /></dd>
          </dl>
          {exchange.response_display.type === "hex_fallback" && <DisplayFallback display={exchange.response_display} />}
          <ProtocolHexViewer bytes={exchange.written_response} document={exchange.response_document} display={exchange.response_display} label="本机应答响应" />
        </section>
      </div>
    </div>
  );
}

function LocalFailureDetail({ detail }: { detail: SocketCaptureDetailViewModel }) {
  if (detail.record.payload.kind !== "local_exchange_failure") return null;
  const failure = detail.record.payload.capture;
  return (
    <div className="space-y-6">
      <Alert status="danger">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{failureTitle[failure.failure_stage]}</Alert.Title>
          <Alert.Description>
            {failure.failure_message} <code className="ml-1 text-xs">{failure.failure_code}</code>
          </Alert.Description>
        </Alert.Content>
      </Alert>
      <section className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
        <div>
          <h2 className="font-semibold">已解析的应用请求</h2>
          <p className="text-xs text-[var(--telemetry-muted)]">应用 → 本机应答</p>
        </div>
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
          <dt>命中规则</dt><dd><Rules ids={failure.matched_request_rule_ids} /></dd>
        </dl>
        <ProtocolHexViewer
          bytes={failure.request_origin}
          document={failure.request_document}
          display={failure.request_display}
          label="失败前解析的应用请求"
        />
      </section>
      <section className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
        <div>
          <h2 className="font-semibold">未完成的本机应答</h2>
          <p className="text-xs text-[var(--telemetry-muted)]">本机应答 → 应用</p>
        </div>
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
          <dt>命中规则</dt><dd><Rules ids={failure.matched_response_rule_ids} /></dd>
        </dl>
        {failure.response_document && <SocketDocumentView document={failure.response_document} />}
        {failure.written_response_prefix.length > 0
          ? <PaginatedHex bytes={failure.written_response_prefix} label="已写出的响应前缀" />
          : <p className="text-sm text-[var(--telemetry-muted)]">未写出响应字节</p>}
      </section>
    </div>
  );
}

export function SocketCaptureDetail(props: SocketCaptureDetailProps) {
  return (
    <Modal isOpen={Boolean(props.selected)} onOpenChange={(open) => { if (!open) props.onClose(); }}>
      <Button className="hidden" aria-hidden="true">打开 Socket 抓包详情</Button>
      <Modal.Backdrop isDismissable>
        <Modal.Container size="cover" scroll="inside">
          <Modal.Dialog>
            <Modal.Header className="items-start gap-1 pr-12 text-left">
              <Modal.Heading className="text-left text-lg font-semibold">Socket 抓包详情</Modal.Heading>
              <p className="max-w-full truncate text-left text-xs text-[var(--telemetry-muted)]">{props.selected?.capture_id ?? "未选择记录"}</p>
              <Modal.CloseTrigger aria-label="关闭 Socket 抓包详情"><Xmark className="size-4" /></Modal.CloseTrigger>
            </Modal.Header>
            <Modal.Body className="min-h-0 space-y-6">
              {props.selected && <CommonMetadata row={props.selected} detail={props.detail} />}
              {(props.error || props.malformed) && (
                <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>{props.malformed ? "详情数据校验失败" : "详情读取失败"}</Alert.Title><Alert.Description>{props.malformed ? "返回详情与当前记录、协议包或字段结构不一致，已停止展示。" : props.error}</Alert.Description></Alert.Content><Button size="sm" variant="outline" onPress={props.onRetry}>重试</Button></Alert>
              )}
              {props.loading && <div className="grid min-h-48 place-items-center"><Spinner aria-label="正在读取 Socket 抓包详情" /></div>}
              {!props.loading && !props.error && !props.malformed && props.detail && (
                props.detail.record.payload.kind === "relay_frame"
                  ? <RelayDetail detail={props.detail} />
                  : props.detail.record.payload.kind === "local_exchange"
                    ? <LocalDetail detail={props.detail} />
                    : <LocalFailureDetail detail={props.detail} />
              )}
            </Modal.Body>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
