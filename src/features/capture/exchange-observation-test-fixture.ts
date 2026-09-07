import type {
  ExchangeObservationPage,
  ExchangeObservationRecord,
} from "@/generated/rust-types";

export function exchangeRecord(): ExchangeObservationRecord {
  return {
    exchange_id: "exchange-1",
    workspace_id: "10000000-0000-0000-0000-000000000001",
    listener_id: "20000000-0000-0000-0000-000000000002",
    runtime_epoch: "30000000-0000-0000-0000-000000000003",
    peer_address: "10.0.0.2:12345",
    protocol: "socket",
    evidence_evicted: false,
    events: [
      { event: "opened", observed_at: "2026-08-22T10:00:00Z" },
      { event: "received", observed_at: "2026-08-22T10:00:01Z", direction: "upstream", context: { protocol: "socket", bytes: [1, 2] }, document: { mti: "0200" }, display: "0200" },
      { event: "sent", observed_at: "2026-08-22T10:00:02Z", direction: "upstream", context: { protocol: "socket", bytes: [3, 4] } },
      { event: "received", observed_at: "2026-08-22T10:00:03Z", direction: "downstream", context: { protocol: "socket", bytes: [5, 6] }, document: { mti: "0210" }, display: "0210" },
      { event: "sent", observed_at: "2026-08-22T10:00:04Z", direction: "downstream", context: { protocol: "socket", bytes: [7, 8] } },
      { event: "closed", observed_at: "2026-08-22T10:00:05Z", outcome: "completed", error: null },
    ],
  };
}

export function activeExchangeRecord(): ExchangeObservationRecord {
  const record = exchangeRecord();
  return {
    ...record,
    exchange_id: "exchange-active",
    events: record.events.filter((event) => event.event !== "closed"),
  };
}

export function failedExchangeRecord(): ExchangeObservationRecord {
  const record = exchangeRecord();
  return {
    ...record,
    exchange_id: "exchange-failed",
    events: [
      ...record.events.slice(0, -1),
      {
        event: "failed",
        observed_at: "2026-08-22T10:00:05Z",
        direction: "upstream",
        stage: "READ_TIMEOUT",
        context: null,
        error: "socket Exchange read timed out",
        external_package_call: null,
      },
      {
        event: "closed",
        observed_at: "2026-08-22T10:00:06Z",
        outcome: "failed",
        error: "Upstream|READ_TIMEOUT: socket Exchange read timed out",
      },
    ],
  };
}

export function httpExchangeRecord(): ExchangeObservationRecord {
  return {
    exchange_id: "exchange-http-1",
    workspace_id: "10000000-0000-0000-0000-000000000001",
    listener_id: "20000000-0000-0000-0000-000000000001",
    runtime_epoch: "30000000-0000-0000-0000-000000000004",
    peer_address: "10.0.0.3:23456",
    protocol: "http",
    evidence_evicted: false,
    events: [
      { event: "opened", observed_at: "2026-08-22T10:01:00Z" },
      { event: "received", observed_at: "2026-08-22T10:01:01Z", direction: "upstream", context: { protocol: "http", header: "POST /pay HTTP/1.1", body: "request", body_is_utf8: true }, document: { amount: 100 }, display: "<p>request</p>" },
      { event: "sent", observed_at: "2026-08-22T10:01:02Z", direction: "upstream", context: { protocol: "http", header: "POST /pay HTTP/1.1", body: "request", body_is_utf8: true } },
      { event: "received", observed_at: "2026-08-22T10:01:03Z", direction: "downstream", context: { protocol: "http", header: "HTTP/1.1 200 OK", body: "response", body_is_utf8: true }, document: { approved: true }, display: "<p>response</p>" },
      { event: "sent", observed_at: "2026-08-22T10:01:04Z", direction: "downstream", context: { protocol: "http", header: "HTTP/1.1 200 OK", body: "response", body_is_utf8: true } },
      { event: "closed", observed_at: "2026-08-22T10:01:05Z", outcome: "completed", error: null },
    ],
  };
}

export function exchangePage(): ExchangeObservationPage {
  return {
    rows: [httpExchangeRecord(), exchangeRecord()],
    page: 1,
    page_size: 50,
    total: 2,
    evicted_records: 0,
    dropped_events: 0,
    ignored_events: 0,
  };
}
