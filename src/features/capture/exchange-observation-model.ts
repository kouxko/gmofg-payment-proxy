import type {
  ExchangeObservationEvent,
  ExchangeObservationQuery,
  ExchangeObservationRecord,
} from "@/generated/rust-types";

export function defaultExchangeObservationQuery(
  workspaceId: string,
  page = 1,
): ExchangeObservationQuery {
  return {
    workspace_id: workspaceId,
    listener_id: null,
    page: { page, page_size: 50 },
  };
}

export function eventRoute(event: ExchangeObservationEvent): string {
  if (event.event === "opened") return "App → Proxy（连接建立）";
  if (event.event === "closed") return "Proxy（连接结束）";
  if (event.event === "received") {
    return event.direction === "upstream" ? "App → Proxy" : "Server → Proxy";
  }
  if (event.event === "sent") {
    return event.direction === "upstream" ? "Proxy → Server" : "Proxy → App";
  }
  if (event.direction === "upstream") return "App / Proxy → Server";
  if (event.direction === "downstream") return "Server / Proxy → App";
  return "Exchange";
}

export function eventLabel(event: ExchangeObservationEvent): string {
  switch (event.event) {
    case "opened": return "连接建立";
    case "received": return "收到";
    case "sent": return "发送";
    case "failed": return `失败 · ${event.stage}`;
    case "closed": return event.outcome === "completed" ? "连接结束" : "连接失败结束";
  }
}

export function openedAt(record: ExchangeObservationRecord): string | undefined {
  return record.events.find((event) => event.event === "opened")?.observed_at;
}

export function finalOutcome(record: ExchangeObservationRecord): string {
  const closed = [...record.events].reverse().find((event) => event.event === "closed");
  if (!closed || closed.event !== "closed") return "连接中";
  return closed.outcome === "completed" ? "已结束" : "失败";
}

export function eventCounts(record: ExchangeObservationRecord) {
  return record.events.reduce(
    (counts, event) => {
      if (event.event === "received") counts.received += 1;
      if (event.event === "sent") counts.sent += 1;
      if (event.event === "failed") counts.failed += 1;
      return counts;
    },
    { received: 0, sent: 0, failed: 0 },
  );
}
