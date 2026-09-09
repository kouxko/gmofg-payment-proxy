import { Alert, Button, Chip, Modal, Spinner } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import type {
  ExchangeContext,
  ExchangeObservationEvent,
  ExchangeObservationRecord,
} from "@/generated/rust-types";
import { formatTimestamp } from "@/lib/format";
import { ProtocolSafeDisplay } from "@/features/shared/protocol-safe-display";
import { SocketByteViewer } from "./socket-byte-viewer";
import { eventLabel, eventRoute } from "./exchange-observation-model";

interface Props {
  selected?: ExchangeObservationRecord;
  detail?: ExchangeObservationRecord;
  error?: string;
  loading: boolean;
  onClose: () => void;
  onRetry: () => void;
  onCreateMockDraft: (exchangeId: string, responseEventIndex: number) => void;
}

function ContextEvidence({ context }: { context: ExchangeContext }) {
  if (context.protocol === "socket") {
    return <SocketByteViewer bytes={context.bytes} label="Socket context" />;
  }
  return <div className="grid gap-3 xl:grid-cols-2">
    <div><h4 className="mb-2 text-sm font-semibold">Header</h4><pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[var(--telemetry-code-bg)] p-3 text-xs">{context.header}</pre></div>
    <div><h4 className="mb-2 text-sm font-semibold">Body</h4><pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[var(--telemetry-code-bg)] p-3 text-xs">{context.body}</pre></div>
  </div>;
}

function EventEvidence({ event }: { event: ExchangeObservationEvent }) {
  if (event.event === "opened") return <p className="text-sm">App connection 已由 Proxy 接受。</p>;
  if (event.event === "closed") return <div className="space-y-2 text-sm"><p>连接状态：{eventLabel(event)}</p>{event.error && <p className="break-all text-danger">{event.error}</p>}</div>;
  if (event.event === "failed") return <div className="space-y-3"><Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>{event.stage}</Alert.Title><Alert.Description>{event.error}</Alert.Description></Alert.Content></Alert>{event.external_package_call && <ExternalPackageFailure call={event.external_package_call} />}{event.context && <ContextEvidence context={event.context} />}</div>;
  if (event.event === "processed") return <div className="space-y-3"><h4 className="text-sm font-semibold">Rule processing changes</h4>{event.changes_truncated && <Alert status="warning"><Alert.Indicator /><Alert.Content><Alert.Title>部分规则变化因观测预算被截断</Alert.Title><Alert.Description>规则处理和最终 Encode 继续完成；此状态只表示观测摘要不完整。</Alert.Description></Alert.Content></Alert>}{event.changes.length === 0 ? <p className="text-xs text-[var(--telemetry-muted)]">No rule changes</p> : event.changes.map((change) => <section className="space-y-1 rounded-lg border p-2" key={change.rule_id}><p className="text-xs"><code>{change.rule_id}</code> · {change.matched ? "matched" : "not matched"}</p><pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded bg-[var(--telemetry-code-bg)] p-2 text-xs">{JSON.stringify(change.operations, null, 2)}</pre></section>)}<h4 className="text-sm font-semibold">Final working Document</h4><pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[var(--telemetry-code-bg)] p-3 text-xs">{JSON.stringify(event.final_document, null, 2)}</pre></div>;
  if (event.event === "encoded") return <div className="space-y-2"><h4 className="text-sm font-semibold">Encode result</h4><ContextEvidence context={event.context} /></div>;
  if (event.event === "sent") return <div className="space-y-2"><h4 className="text-sm font-semibold">Encode / Sent result</h4><ContextEvidence context={event.context} /></div>;
  return <div className="space-y-3">
    <ContextEvidence context={event.context} />
    {event.document != null && <section className="space-y-2" aria-label="Document process evidence">
      <h4 className="text-sm font-semibold">Original Decode Document</h4>
      <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[var(--telemetry-code-bg)] p-3 text-xs">{JSON.stringify(event.document, null, 2)}</pre>
    </section>}
    {event.event === "received" && event.display != null && (
      <div>
        <h4 className="mb-2 text-sm font-semibold">Display</h4>
        <ProtocolSafeDisplay html={event.display} />
      </div>
    )}
  </div>;
}

function ExternalPackageFailure({ call }: { call: NonNullable<Extract<ExchangeObservationEvent, { event: "failed" }>["external_package_call"]> }) {
  return (
    <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-1 rounded-lg border border-danger p-3 text-xs">
      <dt>稳定错误码</dt><dd><code>{call.stable_code ?? "—"}</code></dd>
      <dt>调用方法</dt><dd><code>{call.method}</code></dd>
      <dt>协议包</dt><dd><code>{call.package.id}@{call.package.version}</code></dd>
      {call.remote_message && <><dt>远端消息</dt><dd>{call.remote_message}</dd></>}
      {call.remote_data_summary && <><dt>远端数据形状</dt><dd>{call.remote_data_summary}</dd></>}
    </dl>
  );
}

function Timeline({
  record,
  onCreateMockDraft,
}: {
  record: ExchangeObservationRecord;
  onCreateMockDraft: Props["onCreateMockDraft"];
}) {
  return <ol className="space-y-4" aria-label="Exchange 事件时间线">
    {record.events.map((event, index) => <li key={`${event.observed_at}-${index}`} className="rounded-xl border border-[var(--telemetry-line)] p-4">
      <header className="mb-3 flex flex-wrap items-center gap-2">
        <Chip size="sm" color={event.event === "failed" || (event.event === "closed" && event.outcome !== "completed") ? "danger" : event.event === "closed" ? "success" : "accent"} variant="soft">{eventLabel(event)}</Chip>
        <strong className="text-sm">{eventRoute(event)}</strong>
        <time className="ml-auto text-xs text-[var(--telemetry-muted)]">{formatTimestamp(event.observed_at)}</time>
      </header>
      <EventEvidence event={event} />
      {event.event === "received" && event.direction === "downstream" && event.context.protocol === "http" && (
        <Button
          className="mt-3"
          size="sm"
          variant="outline"
          onPress={() => onCreateMockDraft(record.exchange_id, index)}
        >
          用此响应 Body 创建替换规则
        </Button>
      )}
    </li>)}
  </ol>;
}

export function ExchangeObservationDetail(props: Props) {
  return <Modal isOpen={Boolean(props.selected)} onOpenChange={(open) => { if (!open) props.onClose(); }}>
    <Button className="hidden" aria-hidden="true">打开 Exchange 详情</Button>
    <Modal.Backdrop isDismissable><Modal.Container size="cover" scroll="inside"><Modal.Dialog>
      <Modal.Header className="items-start gap-1 pr-12 text-left">
        <Modal.Heading className="text-left text-lg font-semibold">Exchange 连接详情</Modal.Heading>
        <p className="max-w-full truncate text-left text-xs text-[var(--telemetry-muted)]">{props.selected?.exchange_id ?? "未选择记录"}</p>
        <Modal.CloseTrigger aria-label="关闭 Exchange 详情"><Xmark className="size-4" /></Modal.CloseTrigger>
      </Modal.Header>
      <Modal.Body className="min-h-0 space-y-6">
        {props.detail && <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm"><dt>协议</dt><dd>{props.detail.protocol.toUpperCase()}</dd><dt>对端</dt><dd><code>{props.detail.peer_address}</code></dd><dt>监听器</dt><dd><code>{props.detail.listener_id}</code></dd><dt>Exchange ID</dt><dd><code>{props.detail.exchange_id}</code></dd></dl>}
        {props.detail?.evidence_evicted && <Alert status="warning"><Alert.Indicator /><Alert.Content><Alert.Title>该时间线存在观测淘汰</Alert.Title><Alert.Description>下列事件只代表当前仍保留的有序证据，交易数据面未受影响。</Alert.Description></Alert.Content></Alert>}
        {props.error && <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>详情读取失败</Alert.Title><Alert.Description>{props.error}</Alert.Description></Alert.Content><Button size="sm" variant="outline" onPress={props.onRetry}>重试</Button></Alert>}
        {props.loading && <div className="grid min-h-48 place-items-center"><Spinner aria-label="正在读取 Exchange 详情" /></div>}
        {!props.loading && !props.error && props.detail && <Timeline record={props.detail} onCreateMockDraft={props.onCreateMockDraft} />}
      </Modal.Body>
    </Modal.Dialog></Modal.Container></Modal.Backdrop>
  </Modal>;
}
