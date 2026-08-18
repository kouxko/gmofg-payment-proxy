// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  HttpListenerSettings,
  ListenerProtocolPackageCatalogViewModel,
  ListenerProtocolPackageOptionViewModel,
} from "@/generated/rust-types";
import { HttpProtocolProcessingCard } from "./http-protocol-processing-card";

function option(
  id: string,
  version = "1.0.0",
  overrides: Partial<ListenerProtocolPackageOptionViewModel> = {},
): ListenerProtocolPackageOptionViewModel {
  return {
    package: { id, version },
    name: `${id} ${version}`,
    kind: "http",
    capabilities: {
      upstream: { frame: true, decode: true, encode: true },
      downstream: { frame: true, decode: true, encode: true },
      display: true,
    },
    upstream_schema: {
      id: "http-req",
      version: 1,
      title: "http-req",
      fields: [{ name: "body", label: "body", type: "string" }],
    },
    downstream_schema: {
      id: "http-res",
      version: 1,
      title: "http-res",
      fields: [{ name: "code", label: "code", type: "string" }],
    },
    ...overrides,
  };
}

function catalog(
  options: ListenerProtocolPackageOptionViewModel[] = [],
  installed = options.length,
  unavailable = 0,
  overrides: Partial<ListenerProtocolPackageCatalogViewModel> = {},
) {
  return {
    loading: false,
    refresh: vi.fn().mockResolvedValue(undefined),
    data: {
      options,
      installed_version_count: installed,
      unavailable_version_count: unavailable,
      recommended_package: null,
      ...overrides,
    },
  };
}

function settings(mode: "plain" | "protocol" = "plain"): HttpListenerSettings {
  return {
    authentication: { mode: "none" },
    mitm: {
      enabled: false,
      authority_allowlist: [],
      root_ca: null,
      maximum_cached_leaf_certificates: 256,
    },
    downstream_tls: {
      enabled: false,
      server_identity: null,
      dynamic_sni_allowlist: [],
      client_authentication: { mode: "disabled" },
    },
    request_body_codec: "auto",
    response_body_codec: "auto",
    body_processing: mode === "plain" ? { mode: "plain" } : {
      mode: "protocol",
      package: { id: "http-json", version: "1.0.0" },
    },
    fixed_server: null,
  };
}

describe("HttpProtocolProcessingCard", () => {
  it("allows selecting protocol package and persists exact identity", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    const optionValue = option("http-json", "1.0.0");
    render(<HttpProtocolProcessingCard settings={settings()} catalog={catalog([optionValue], 1)} locked={false} onChange={onChange} />);

    await user.click(screen.getByLabelText("HTTP 协议处理方案"));
    await user.click(await screen.findByRole("option", { name: "http-json 1.0.0 · 1.0.0" }));

    expect(onChange).toHaveBeenCalledWith({
      body_processing: { mode: "protocol", package: { id: "http-json", version: "1.0.0" } },
    });
    expect(screen.getByRole("status")).toHaveTextContent("已选择");
  });

  it("returns to plain mode", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<HttpProtocolProcessingCard settings={settings("protocol")} catalog={catalog([option("http-json", "1.0.0")], 1)} locked={false} onChange={onChange} />);

    await user.click(screen.getByLabelText("HTTP 协议处理方案"));
    await user.click(await screen.findByRole("option", { name: "不使用协议包（明文透传）" }));
    expect(onChange).toHaveBeenCalledWith({ body_processing: { mode: "plain" } });
  });

  it("does not list socket package option in HTTP selector", async () => {
    const user = userEvent.setup();
    render(
      <HttpProtocolProcessingCard
        settings={settings()}
        catalog={catalog([
          option("http-json", "1.0.0"),
          option("socket-example", "2.0.0", { kind: "socket", name: "Socket JSON" }),
        ], 2)}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    await user.click(screen.getByLabelText("HTTP 协议处理方案"));
    expect(await screen.findByRole("option", { name: "http-json 1.0.0 · 1.0.0" })).toBeVisible();
    expect(screen.queryByRole("option", { name: /Socket JSON/ })).not.toBeInTheDocument();
  });

  it.each([
    { label: "loading", state: { ...catalog([option("http-json")]), loading: true } },
    { label: "error", state: { ...catalog([option("http-json")]), error: "读取失败" } },
  ])("fails closed while the catalog is $label", ({ state }) => {
    render(
      <HttpProtocolProcessingCard
        settings={settings()}
        catalog={state}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("HTTP 协议处理方案")).toBeDisabled();
    expect(screen.queryByText("当前 Body 协议处理已不可用")).not.toBeInTheDocument();
  });

  it("keeps an exact HTTP package selection available", async () => {
    const user = userEvent.setup();
    render(
      <HttpProtocolProcessingCard
        settings={settings("protocol")}
        catalog={catalog([
          option("http-json", "1.0.0"),
          option("http-json", "2.0.0"),
        ], 2)}
        locked={false}
        onChange={vi.fn()}
      />,
    );

    await user.click(screen.getByLabelText("HTTP 协议处理方案"));
    expect(screen.queryByRole("option", { name: "当前选择（不可用）" })).not.toBeInTheDocument();
    expect(await screen.findByRole("option", { name: "http-json 1.0.0 · 1.0.0" })).toBeVisible();
    expect(screen.getByRole("option", { name: "http-json 2.0.0 · 2.0.0" })).toBeVisible();
  });
});
