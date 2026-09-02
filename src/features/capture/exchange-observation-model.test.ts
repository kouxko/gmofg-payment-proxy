import { describe, expect, it } from "vitest";
import {
  activeExchangeRecord,
  exchangeRecord,
  failedExchangeRecord,
} from "./exchange-observation-test-fixture";
import {
  defaultExchangeObservationQuery,
  connectionStatus,
  eventCounts,
  eventRoute,
} from "./exchange-observation-model";

describe("exchange observation model", () => {
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
