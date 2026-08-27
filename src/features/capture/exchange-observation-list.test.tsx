import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ExchangeObservationList } from "./exchange-observation-list";
import { exchangePage } from "./exchange-observation-test-fixture";

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
});
