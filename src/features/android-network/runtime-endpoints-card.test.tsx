// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AndroidNetworkEndpointSnapshotViewModel } from "@/generated/rust-types";
import { RuntimeEndpointsCard } from "./runtime-endpoints-card";

const snapshot: AndroidNetworkEndpointSnapshotViewModel = {
  configured_profile_id: "selected-profile",
  configured: [{
    profile_id: "selected-profile",
    original_destination: "configured.example.test",
    original_ports: [443],
    listener_id: "configured-listener",
    listener_name: "配置入口",
    listener_bind_address: "0.0.0.0",
    listener_port: 16627,
  }],
  runtime_owner: {
    serial: "owner-a",
    epoch: "11111111-1111-4111-8111-111111111111",
    mode: "lan",
    profile_id: "owner-profile",
    state: "active",
    source: "start",
    transition_reason: "activation_confirmed",
    updated_at: "2026-08-18T01:00:00Z",
  },
  runtime: [{
    serial: "owner-a",
    epoch: "11111111-1111-4111-8111-111111111111",
    mode: "lan",
    original_destination: "runtime.example.test",
    original_ports: [443, 8443],
    resolved_original_ips: ["203.0.113.8"],
    listener_id: "runtime-listener",
    listener_name: "实际入口",
    desktop_listener_port: 16627,
    proxy_host: "10.0.34.48",
    proxy_port: 16627,
    resolved_at: "2026-08-18T01:02:03Z",
    health: "healthy",
  }],
};

describe("Android runtime endpoint card", () => {
  it("separates selected-profile configuration from owner runtime facts", () => {
    render(<RuntimeEndpointsCard snapshot={snapshot} loading={false} />);

    const configured = screen.getByLabelText("方案配置端点");
    const runtime = screen.getByLabelText("实际运行端点");
    expect(configured).toHaveTextContent("configured.example.test:443");
    expect(configured).toHaveTextContent("配置入口（configured-listener）");
    expect(configured).toHaveTextContent("0.0.0.0:16627");
    expect(runtime).toHaveTextContent("owner-a");
    expect(runtime).toHaveTextContent("局域网");
    expect(runtime).toHaveTextContent("10.0.34.48:16627");
    expect(runtime).toHaveTextContent("实际入口（runtime-listener）");
    expect(runtime).toHaveTextContent("2026-08-18T01:02:03Z");
    expect(runtime).toHaveTextContent("健康");
  });

  it("reports successful LAN endpoint reapply without hiding runtime facts", () => {
    render(<RuntimeEndpointsCard snapshot={{
      ...snapshot,
      runtime_owner: {
        ...snapshot.runtime_owner!,
        transition_reason: "lan_endpoint_reapplied",
      },
    }} loading={false} />);

    expect(screen.getByText("LAN 地址变化后，实际运行端点已重新应用。")).toBeVisible();
    expect(screen.getByLabelText("实际运行端点")).toHaveTextContent("10.0.34.48:16627");
  });

  it("reports a faulted LAN endpoint as failed recovery", () => {
    render(<RuntimeEndpointsCard snapshot={{
      ...snapshot,
      runtime_owner: {
        ...snapshot.runtime_owner!,
        state: "faulted",
        transition_reason: "lan_endpoint_faulted",
      },
      runtime: [{ ...snapshot.runtime[0]!, health: "faulted" }],
    }} loading={false} />);

    expect(screen.getByText(/LAN 地址变化后无法恢复实际运行端点/)).toBeVisible();
    expect(screen.getByLabelText("实际运行端点")).toHaveTextContent("故障");
  });

  it("does not describe a healthy Reverse endpoint as rebuilt or reapplied", () => {
    render(<RuntimeEndpointsCard snapshot={{
      ...snapshot,
      runtime_owner: {
        ...snapshot.runtime_owner!,
        mode: "adb_reverse",
        transition_reason: "activation_confirmed",
      },
      runtime: [{ ...snapshot.runtime[0]!, mode: "adb_reverse" }],
    }} loading={false} />);

    const runtime = screen.getByLabelText("实际运行端点");
    expect(runtime).toHaveTextContent("USB / ADB Reverse");
    expect(runtime).toHaveTextContent("健康");
    expect(runtime).not.toHaveTextContent(/重建|重新应用/);
  });
});
