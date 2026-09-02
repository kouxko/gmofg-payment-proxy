import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ExchangeObservationList } from "./exchange-observation-list";
import {
  activeExchangeRecord,
  exchangePage,
  exchangeRecord,
  failedExchangeRecord,
} from "./exchange-observation-test-fixture";

describe("ExchangeObservationList", () => {
  it("shows HTTP and Socket connections in one table with loss counters", () => {
    const page = { ...exchangePage(), evicted_records: 2, dropped_events: 4, ignored_events: 3 };
    render(<ExchangeObservationList page={page} loading={false} onSelect={vi.fn()} onPage={vi.fn()} onRetry={vi.fn()} onClear={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "运行记录" })).toBeVisible();
    expect(screen.getByRole("grid", { name: "HTTP 与 Socket 运行记录" })).toBeVisible();
    expect(screen.getByText("HTTP")).toBeVisible();
    expect(screen.getByText("SOCKET")).toBeVisible();
    expect(screen.queryByText("Exchange 连接时间线")).toBeNull();
    expect(screen.getByText(/当前工作区已淘汰连接 2 条；producer 队列丢弃 4 条；consumer\/store 忽略 3 条/)).toBeVisible();
    expect(screen.getAllByText("2 / 2 / 0")).toHaveLength(2);
  });

  it("labels the result column as connection status and renders all three lifecycle states", () => {
    const page = {
      ...exchangePage(),
      rows: [activeExchangeRecord(), exchangeRecord(), failedExchangeRecord()],
      total: 3,
    };

    render(<ExchangeObservationList page={page} loading={false} onSelect={vi.fn()} onPage={vi.fn()} onRetry={vi.fn()} onClear={vi.fn()} />);

    expect(screen.getByRole("columnheader", { name: "连接状态" })).toBeVisible();
    const activeStatus = screen.getByText("保持连接");
    expect(activeStatus).toBeVisible();
    expect(activeStatus.closest('[data-slot="chip"]')).toHaveClass("chip--accent");
    expect(activeStatus.closest('[data-slot="chip"]')).not.toHaveClass("chip--warning");
    expect(screen.getByText("正常结束")).toBeVisible();
    expect(screen.getByText("异常结束")).toBeVisible();
    expect(screen.queryByRole("columnheader", { name: "结果" })).not.toBeInTheDocument();
  });
});
