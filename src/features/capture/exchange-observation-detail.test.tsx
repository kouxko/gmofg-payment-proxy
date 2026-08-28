import { render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ExchangeObservationDetail } from "./exchange-observation-detail";
import {
  exchangeRecord,
  failedExchangeRecord,
  httpExchangeRecord,
} from "./exchange-observation-test-fixture";

describe("ExchangeObservationDetail", () => {
  it("renders the connection Vec in order with four distinct routes", () => {
    const record = exchangeRecord();
    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);
    const timeline = screen.getByRole("list", { name: "Exchange 事件时间线" });
    const items = within(timeline).getAllByRole("listitem");
    expect(items).toHaveLength(6);
    expect(items[1]).toHaveTextContent("App → Proxy");
    expect(items[2]).toHaveTextContent("Proxy → Server");
    expect(items[3]).toHaveTextContent("Server → Proxy");
    expect(items[4]).toHaveTextContent("Proxy → App");
  });

  it("shows explicit eviction evidence", () => {
    const record = { ...exchangeRecord(), evidence_evicted: true };
    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);
    expect(screen.getByText("该时间线存在观测淘汰")).toBeVisible();
  });

  it("hides the internal Document and safely renders protocol Display HTML", async () => {
    const record = exchangeRecord();
    record.events = record.events.map((event) => event.event === "received"
      ? {
          ...event,
          document: { internal_only: "must not render" },
          display: "<section><h3>ISO 8583</h3><table><tbody><tr><th>MTI</th><td>0200</td></tr></tbody></table><script>unsafe()</script></section>",
        }
      : event);

    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);

    expect(screen.queryByText("Document")).not.toBeInTheDocument();
    expect(screen.queryByText(/must not render/)).not.toBeInTheDocument();
    const frames = await screen.findAllByTitle("协议包安全展示");
    expect(frames).toHaveLength(2);
    await waitFor(() => expect(frames[0]).toHaveAttribute("srcdoc"));
    expect(frames[0]).toHaveAttribute("sandbox", "");
    expect(frames[0].getAttribute("srcdoc")).toContain("ISO 8583");
    expect(frames[0].getAttribute("srcdoc")).not.toContain("unsafe()");
  });

  it("offers the action on a server HTTP response and passes the exact event index", async () => {
    const onCreateMockDraft = vi.fn();
    const record = httpExchangeRecord();
    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={onCreateMockDraft} />);

    const action = screen.getByRole("button", { name: "用此服务器响应创建 Mock 草稿" });
    action.click();

    expect(onCreateMockDraft).toHaveBeenCalledWith("exchange-http-1", 3);
  });

  it("describes a completed close as a normal connection ending", () => {
    const record = exchangeRecord();
    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);

    expect(screen.getByText("正常结束")).toBeVisible();
    expect(screen.getByText("连接状态：正常结束")).toBeVisible();
  });

  it("describes an error close as abnormal and preserves the original error", () => {
    const record = failedExchangeRecord();
    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);

    expect(screen.getByText("异常结束")).toBeVisible();
    expect(screen.getByText("连接状态：异常结束")).toBeVisible();
    expect(screen.getByText("Upstream|READ_TIMEOUT: socket Exchange read timed out")).toBeVisible();
  });
});
