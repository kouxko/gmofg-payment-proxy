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

  it("renders typed rule processing, final Document, and Encode evidence", async () => {
    const record = exchangeRecord();
    record.events = [
      record.events[0],
      { ...record.events[1], document: { amount: 100 }, display: "<section><h3>ISO 8583</h3><script>unsafe()</script></section>" },
      {
        event: "processed", observed_at: "2026-08-22T10:00:01.500Z", direction: "upstream",
        changes: [{ rule_id: "40000000-0000-0000-0000-000000000004", matched: true, operations: [{ kind: "set", path: "/amount" }] }],
        changes_truncated: true,
        final_document: { amount: 120 },
      },
      { event: "encoded", observed_at: "2026-08-22T10:00:01.750Z", direction: "upstream", context: { protocol: "socket", bytes: [3, 4] } },
      ...record.events.slice(2),
    ] as typeof record.events;

    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);

    expect(screen.getAllByText("Original Decode Document")).toHaveLength(2);
    expect(screen.getByText("Rule processing changes")).toBeVisible();
    expect(screen.getByText("部分规则变化因观测预算被截断")).toBeVisible();
    expect(screen.getByText(/40000000-0000-0000-0000-000000000004/)).toBeVisible();
    expect(screen.getByText(/\"path\": \"\/amount\"/)).toBeVisible();
    expect(screen.getByText(/\"amount\": 120/)).toBeVisible();
    expect(screen.getByText("Final working Document")).toBeVisible();
    expect(screen.getByText("Encode result")).toBeVisible();
    expect(screen.queryByText(/contract 未提供/)).not.toBeInTheDocument();
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

  it("shows the Rust stable code for a failed package stage", () => {
    const record = failedExchangeRecord();
    const failed = record.events.find((event) => event.event === "failed");
    if (!failed || failed.event !== "failed") throw new Error("failed fixture is invalid");
    failed.stage = "decode";
    failed.external_package_call = {
      package: { id: "com.example.payment", version: "1.0.0" },
      direction: "upstream",
      stage: "decode",
      method: "hooks.upstream.decode",
      request_id: "rpc-7",
      remote_code: -32001,
      stable_code: "PACKAGE_DECODE_FAILED",
      remote_message: "decode rejected",
      remote_data_summary: "object(fields=1)",
    };

    render(<ExchangeObservationDetail selected={record} detail={record} loading={false} onClose={vi.fn()} onRetry={vi.fn()} onCreateMockDraft={vi.fn()} />);

    expect(screen.getByText("PACKAGE_DECODE_FAILED")).toBeVisible();
    expect(screen.getByText("hooks.upstream.decode")).toBeVisible();
  });
});
