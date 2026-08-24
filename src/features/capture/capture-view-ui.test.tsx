// @vitest-environment jsdom

/** 抓包页只保留一张同时承载 HTTP 与 Socket 的 Exchange 运行记录表。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CaptureView } from "./capture-view";

vi.mock("./exchange-observation-view", () => ({
  ExchangeObservationView: () => (
    <section aria-label="统一运行记录工作区">
      <h2>运行记录</h2>
      <table aria-label="HTTP 与 Socket 运行记录">
        <tbody>
          <tr><td>HTTP</td></tr>
          <tr><td>SOCKET</td></tr>
        </tbody>
      </table>
    </section>
  ),
}));

describe("CaptureView unified records", () => {
  it("renders one shared region and no protocol-specific capture areas", () => {
    render(<CaptureView />);

    expect(screen.getAllByRole("region")).toHaveLength(1);
    expect(screen.getByRole("region", { name: "统一运行记录工作区" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "运行记录" })).toBeVisible();
    expect(screen.getByRole("table", { name: "HTTP 与 Socket 运行记录" })).toBeVisible();
    expect(screen.getByText("HTTP")).toBeVisible();
    expect(screen.getByText("SOCKET")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "HTTP 抓包" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Socket 运行记录" })).toBeNull();
  });
});
