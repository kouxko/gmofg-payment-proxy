import { describe, expect, it } from "vitest";
import {
  activeExchangeRecord,
  exchangeRecord,
  failedExchangeRecord,
  httpExchangeRecord,
} from "./exchange-observation-test-fixture";
import {
  defaultExchangeObservationQuery,
  connectionStatus,
  eventCounts,
  eventRoute,
  latestAppRequestTarget,
} from "./exchange-observation-model";

describe("exchange observation model", () => {
  it("shows the latest original App target without response or rewritten target interference", () => {
    const record = httpExchangeRecord();
    const original = `/api/pay?token=a%2Fb+c&empty=&id=1&id=2&long=${"x".repeat(2048)}`;
    record.events.push(
      { event: "received", observed_at: "2026-09-09T01:00:00Z", direction: "upstream", context: { protocol: "http", header: `POST ${original} HTTP/1.1\r\nHost: example.test\r\n\r\n`, body: "", body_is_utf8: true }, document: null, display: null },
      { event: "encoded", observed_at: "2026-09-09T01:00:01Z", direction: "upstream", context: { protocol: "http", header: "POST /rewritten HTTP/1.1", body: "", body_is_utf8: true } },
      { event: "sent", observed_at: "2026-09-09T01:00:02Z", direction: "upstream", context: { protocol: "http", header: "POST /rewritten HTTP/1.1", body: "", body_is_utf8: true } },
      ...record.events.slice(3),
    );
    const before = structuredClone(record);
    expect(latestAppRequestTarget(record)).toBe(original);
    expect(record).toEqual(before);
  });

  it("does not invent a target for Socket or a connection before its first App request", () => {
    expect(latestAppRequestTarget(exchangeRecord())).toBeUndefined();
    const record = httpExchangeRecord();
    record.events = record.events.filter((event) => event.event !== "received" || event.direction !== "upstream");
    expect(latestAppRequestTarget(record)).toBeUndefined();
  });

  it("does not fall back to an older target when the latest request line is unavailable", () => {
    const record = httpExchangeRecord();
    record.events.push({ event: "received", observed_at: "2026-09-09T01:00:00Z", direction: "upstream", context: { protocol: "http", header: "", body: "", body_is_utf8: true }, document: null, display: null });
    expect(latestAppRequestTarget(record)).toBeUndefined();
  });

  it.each(["/", "/pay?", "/pay?x=%2f&x=+", "http://example.test/pay?x=1", "*"])("preserves the observed request target %s verbatim", (target) => {
    const record = httpExchangeRecord();
    const event = record.events[1];
    if (event.event !== "received" || event.context.protocol !== "http") throw new Error("invalid HTTP fixture");
    event.context.header = `GET ${target} HTTP/1.1\nHost: example.test\n`;
    expect(latestAppRequestTarget(record)).toBe(target);
  });

  it("builds a workspace-scoped bounded query", () => {
    expect(defaultExchangeObservationQuery("workspace-1", 3)).toEqual({
      workspace_id: "workspace-1",
      listener_id: null,
      page: { page: 3, page_size: 50 },
    });
  });

  it("maps all four network facts without collapsing direction", () => {
    const [, appReceived, serverSent, serverReceived, appSent] = exchangeRecord().events;
    expect(eventRoute(appReceived)).toBe("App → Proxy");
    expect(eventRoute(serverSent)).toBe("Proxy → Server");
    expect(eventRoute(serverReceived)).toBe("Server → Proxy");
    expect(eventRoute(appSent)).toBe("Proxy → App");
  });

  it("preserves every appended event so a later response cannot overwrite an earlier one", () => {
    const record = exchangeRecord();
    record.events.splice(5, 0, ...record.events.slice(1, 5));
    expect(eventCounts(record)).toEqual({ received: 4, sent: 4, failed: 0 });
    expect(record.events).toHaveLength(10);
    expect(connectionStatus(record)).toBe("正常结束");
  });

  it("reports a Socket exchange without a closed event as an active connection", () => {
    expect(connectionStatus(activeExchangeRecord())).toBe("保持连接");
  });

  it("reports a completed closed event as a normal connection ending", () => {
    expect(connectionStatus(exchangeRecord())).toBe("正常结束");
  });

  it("reports an error closed event as an abnormal connection ending", () => {
    expect(connectionStatus(failedExchangeRecord())).toBe("异常结束");
  });
});
