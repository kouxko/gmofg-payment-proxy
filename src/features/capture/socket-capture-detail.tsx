import { Alert, Button, Chip, Modal, Spinner } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import type {
  SocketCaptureDetailViewModel,
  SocketCaptureRowViewModel,
  SocketDisplayFallbackReason,
  SocketWriteKind,
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
  encode_disabled: "Encode 未启用，因此未调用 Display",
  not_declared: "协议包未声明 Display，默认显示 Hex",
  entry_point_failed: "Display 执行失败，默认显示 Hex",
  resource_limit_exceeded: "Display 超出脚本资源限制，默认显示 Hex",
};

function WriteKind({ kind }: { kind: SocketWriteKind }) {
  return <Chip size="sm" color={kind === "encoded" ? "accent" : "default"} variant="soft">{kind === "encoded" ? "Encoded" : "Raw Echo"}</Chip>;
}

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
      <dt>Capture</dt><dd className="break-all font-mono text-xs">{row.capture_id}</dd>
      <dt>Peer</dt><dd className="break-all font-mono text-xs">{detail?.record.peer_address ?? "正在读取…"}</dd>
      <dt>协议包</dt><dd><code className="text-xs">{packageLabel(row.package)}</code></dd>
      <dt>Schema</dt><dd><code className="text-xs">{schemaLabel(row.schema)}</code></dd>
      <dt>连接</dt><dd className="break-all font-mono text-xs">{row.connection_id}</dd>
      <dt>Session</dt><dd className="break-all font-mono text-xs">{row.session_id}</dd>
      <dt>入口</dt><dd className="break-all font-mono text-xs">{row.listener_id}</dd>
      <dt>数据量</dt><dd>Origin {formatBytes(row.origin_size_bytes)} · Written {formatBytes(row.written_size_bytes)} · Logical {formatBytes(row.logical_size_bytes)}</dd>
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
        <dt>Decode</dt><dd>{frame.decode_enabled ? "已启用" : "未启用"}</dd>
        <dt>Encode</dt><dd>{frame.encode_enabled ? "已启用" : "未启用"}</dd>
        <dt>写出来源</dt><dd><WriteKind kind={frame.write_kind} /></dd>
        <dt>命中规则</dt><dd><Rules ids={frame.matched_rule_ids} /></dd>
      </dl>
      <section className="space-y-3">
        <h2 className="font-semibold">Origin 原始 Frame</h2>
        <PaginatedHex bytes={frame.origin} label="Relay Origin" />
      </section>
      <section className="space-y-3">
        <h2 className="font-semibold">Document</h2>
        {frame.document ? <SocketDocumentView document={frame.document} /> : (
          <p className="text-sm text-[var(--telemetry-muted)]">Decode 未启用，没有 Document。</p>
        )}
      </section>
      <section className="space-y-3">
        <h2 className="font-semibold">Written 写出字节</h2>
        {frame.display.type === "hex_fallback" && <DisplayFallback display={frame.display} />}
        <ProtocolHexViewer bytes={frame.written} display={frame.display} label="Relay Written" decodeDisabled={!frame.decode_enabled} />
      </section>
    </div>
  );
}

function LocalDetail({ detail }: { detail: SocketCaptureDetailViewModel }) {
  if (detail.record.payload.kind !== "local_exchange") return null;
  const exchange = detail.record.payload.capture;
  return (
    <div className="space-y-3">
      <p className="text-sm">关联 Exchange：<code>{exchange.exchange_id}</code></p>
      <div className="grid gap-6 xl:grid-cols-2">
        <section className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
          <div><h2 className="font-semibold">LocalResponder Request</h2><p className="text-xs text-[var(--telemetry-muted)]">App → LocalResponder</p></div>
          <p className="text-sm">Decode：<span>{exchange.request_decode_enabled ? "已启用" : "未启用（没有 Document）"}</span></p>
          <ProtocolHexViewer bytes={exchange.request_origin} document={exchange.request_document} label="Local Request" decodeDisabled={!exchange.request_decode_enabled} preferDocument />
        </section>
        <section className="space-y-4 rounded-xl border border-[var(--telemetry-line)] p-4">
          <div><h2 className="font-semibold">LocalResponder Response</h2><p className="text-xs text-[var(--telemetry-muted)]">LocalResponder → App</p></div>
          <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
            <dt>Encode</dt><dd>{exchange.response_encode_enabled ? "已启用" : "未启用"}</dd>
            <dt>写出来源</dt><dd><WriteKind kind={exchange.response_write_kind} /></dd>
            <dt>命中规则</dt><dd><Rules ids={exchange.matched_downstream_rule_ids} /></dd>
          </dl>
          {exchange.response_display.type === "hex_fallback" && <DisplayFallback display={exchange.response_display} />}
          <ProtocolHexViewer bytes={exchange.written_response} document={exchange.response_document} display={exchange.response_display} label="Local Response" />
        </section>
      </div>
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
                <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>{props.malformed ? "详情数据校验失败" : "详情读取失败"}</Alert.Title><Alert.Description>{props.malformed ? "返回详情与当前记录、协议包或 Schema 不一致，已停止展示。" : props.error}</Alert.Description></Alert.Content><Button size="sm" variant="outline" onPress={props.onRetry}>重试</Button></Alert>
              )}
              {props.loading && <div className="grid min-h-48 place-items-center"><Spinner aria-label="正在读取 Socket 抓包详情" /></div>}
              {!props.loading && !props.error && !props.malformed && props.detail && (
                props.detail.record.payload.kind === "relay_frame" ? <RelayDetail detail={props.detail} /> : <LocalDetail detail={props.detail} />
              )}
            </Modal.Body>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  );
}
