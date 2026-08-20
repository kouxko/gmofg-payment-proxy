// @vitest-environment jsdom

/** 转发报文与本机应答详情的准确展示和 HTTP 隔离测试。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  SocketCaptureDetailViewModel,
  SocketCaptureDocument,
  SocketCaptureRowViewModel,
} from "@/generated/rust-types";
import { SocketCaptureDetail } from "./socket-capture-detail";

const documentFixture = {
  schema: {
    id: "payment-message",
    version: 3,
    title: "Payment message",
    fields: [{ name: "amount", type: "int", label: "Amount" }],
  },
  values: [{ type: "int", value: "9007199254740993" }],
} satisfies SocketCaptureDocument;

function relayRow(): SocketCaptureRowViewModel {
  return {
    capture_id: "11111111-1111-4111-8111-111111111111",
    runtime_epoch: "22222222-2222-4222-8222-222222222222",
    session_id: "44444444-4444-4444-8444-444444444444",
    connection_id: "44444444-4444-4444-8444-444444444444",
    listener_id: "55555555-5555-4555-8555-555555555555",
    occurred_at: "2026-08-15T09:00:00Z",
    completed_at: "2026-08-15T09:00:01Z",
    kind: "relay_frame",
    direction: "downstream",
    package: { id: "iso8583", version: "2.0.0" },
    schema: { id: "payment-message", version: 3 },
    origin_size_bytes: 2,
    written_size_bytes: 2,
    logical_size_bytes: 768,
    matched_rule_ids: ["66666666-6666-4666-8666-666666666666"],
    failure: null,
  };
}

function relayDetail(
  row: SocketCaptureRowViewModel = relayRow(),
): SocketCaptureDetailViewModel {
  return {
    record: {
      capture_id: row.capture_id,
      runtime_epoch: row.runtime_epoch,
      workspace_id: "77777777-7777-4777-8777-777777777777",
      listener_id: row.listener_id,
      session_id: row.session_id,
      connection_id: row.connection_id,
      peer_address: "127.0.0.1:43000",
      occurred_at: row.occurred_at,
      completed_at: row.completed_at,
      payload: {
        kind: "relay_frame",
        capture: {
          direction: "downstream",
          package: row.package,
          schema: row.schema,
          origin: [0x30, 0x32],
          stages: [
            { stage: "upstream_to_proxy", matched_rule_ids: row.matched_rule_ids, document: documentFixture },
            { stage: "proxy_to_app", matched_rule_ids: [], document: documentFixture },
          ],
          written: [0x30, 0x33],
          display: { type: "untrusted_html", html: "<p>Response 0210</p>" },
        },
      },
    },
  } satisfies SocketCaptureDetailViewModel;
}

function localRow(): SocketCaptureRowViewModel {
  return {
    ...relayRow(),
    capture_id: "88888888-8888-4888-8888-888888888888",
    kind: "local_exchange",
    direction: null,
    origin_size_bytes: 1,
    written_size_bytes: 1,
    matched_rule_ids: ["99999999-9999-4999-8999-999999999999"],
  };
}

function localDetail(
  row: SocketCaptureRowViewModel = localRow(),
): SocketCaptureDetailViewModel {
  return {
    record: {
      capture_id: row.capture_id,
      runtime_epoch: row.runtime_epoch,
      workspace_id: "77777777-7777-4777-8777-777777777777",
      listener_id: row.listener_id,
      session_id: row.session_id,
      connection_id: row.connection_id,
      peer_address: "127.0.0.1:44000",
      occurred_at: row.occurred_at,
      completed_at: row.completed_at,
      payload: {
        kind: "local_exchange",
        capture: {
          exchange_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          package: row.package,
          request_schema: row.schema,
          response_schema: row.schema,
          request_origin: [0x30],
          request_document: documentFixture,
          request_display: { type: "untrusted_html", html: "<p>Local request</p>" },
          response_document: documentFixture,
          matched_request_rule_ids: [],
          matched_response_rule_ids: row.matched_rule_ids,
          written_response: [0x31],
          response_display: { type: "untrusted_html", html: "<p>Local approved</p>" },
        },
      },
    },
  } satisfies SocketCaptureDetailViewModel;
}

function failedLocalRow(): SocketCaptureRowViewModel {
  return {
    ...localRow(),
    written_size_bytes: 0,
    failure: {
      stage: "response_encode",
      code: "ENCODE_FAILED",
      message: "响应报文生成失败，请检查代理→应用规则是否补齐协议要求的字段。",
    },
  };
}

function failedLocalDetail(row: SocketCaptureRowViewModel = failedLocalRow()): SocketCaptureDetailViewModel {
  return {
    record: {
      ...localDetail(row).record,
      payload: {
        kind: "local_exchange_failure",
        capture: {
          exchange_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          package: row.package,
          request_schema: row.schema,
          response_schema: row.schema,
          request_origin: [0x30],
          request_document: documentFixture,
          request_display: { type: "untrusted_html", html: "<p>Local request</p>" },
          matched_request_rule_ids: [],
          matched_response_rule_ids: row.matched_rule_ids,
          response_document: null,
          failure_stage: "response_encode",
          failure_code: "ENCODE_FAILED",
          failure_message: "响应报文生成失败，请检查代理→应用规则是否补齐协议要求的字段。",
          written_response_prefix: [],
        },
      },
    },
  };
}

function renderDetail(
  row: SocketCaptureRowViewModel,
  detail: SocketCaptureDetailViewModel | undefined,
  overrides: Partial<React.ComponentProps<typeof SocketCaptureDetail>> = {},
) {
  const props = {
    selected: row,
    detail,
    error: undefined,
    malformed: false,
    loading: false,
    onClose: vi.fn(),
    onRetry: vi.fn(),
    ...overrides,
  } satisfies React.ComponentProps<typeof SocketCaptureDetail>;
  render(<SocketCaptureDetail {...props} />);
  return props;
}

describe("SocketCaptureDetail Relay", () => {
  it("shows exact relay direction, identities, bytes, parsed fields and matched rules", () => {
    const row = relayRow();
    renderDetail(row, relayDetail(row));

    const dialog = screen.getByRole("dialog", { name: "Socket 抓包详情" });
    expect(within(dialog).getByText("Server → App")).toBeVisible();
    expect(within(dialog).getByText("iso8583@2.0.0")).toBeVisible();
    expect(within(dialog).getByText("payment-message v3")).toBeVisible();
    expect(within(dialog).getAllByText("9007199254740993")).toHaveLength(2);
    expect(
      within(dialog).getByText("66666666-6666-4666-8666-666666666666"),
    ).toBeVisible();
    expect(within(dialog).getByRole("region", { name: "转发原始报文" })).toBeVisible();
    expect(within(dialog).getByText("Decode → 两段规则 → Encode")).toBeVisible();
  });

  it("defaults a failed protocol display to Hex", () => {
    const row = relayRow();
    const detail = relayDetail(row);
    if (detail.record.payload.kind !== "relay_frame") throw new Error("relay fixture");
    detail.record.payload.capture.display = {
      type: "hex_fallback",
      reason: "entry_point_failed",
      diagnostic: { code: "DISPLAY_FAILED", message: "协议视图生成失败" },
    };
    renderDetail(row, detail);

    expect(screen.getByText("协议视图生成失败，默认显示 Hex")).toBeVisible();
    expect(screen.getByRole("tab", { name: "Hex" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("shows a stable protocol-view failure diagnostic without exposing script text", () => {
    const row = relayRow();
    const detail = relayDetail(row);
    if (detail.record.payload.kind !== "relay_frame") throw new Error("relay fixture");
    detail.record.payload.capture.display = {
      type: "hex_fallback",
      reason: "entry_point_failed",
      diagnostic: { code: "DISPLAY_FAILED", message: "协议展示失败" },
    };
    renderDetail(row, detail);

    expect(screen.getByText("协议视图生成失败，默认显示 Hex")).toBeVisible();
    expect(screen.getByText("DISPLAY_FAILED")).toBeVisible();
    expect(screen.getByText(/协议展示失败/)).toBeVisible();
  });

  it("shows both rule-stage snapshots and empty rule state explicitly", () => {
    const row = relayRow();
    const detail = relayDetail(row);
    if (detail.record.payload.kind !== "relay_frame") throw new Error("relay fixture");
    detail.record.payload.capture.stages.forEach((stage) => { stage.matched_rule_ids = []; });
    renderDetail(row, detail);

    expect(screen.getByText("上游服务 → 代理")).toBeVisible();
    expect(screen.getByText("代理 → 应用")).toBeVisible();
    expect(screen.getAllByText("无规则命中")).toHaveLength(2);
  });
});

describe("SocketCaptureDetail local response", () => {
  it("associates Request and Response under the same stable exchange ID", () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const dialog = screen.getByRole("dialog", { name: "Socket 抓包详情" });
    expect(within(dialog).getByRole("heading", { name: "本机应答请求" })).toBeVisible();
    expect(within(dialog).getByRole("heading", { name: "本机应答响应" })).toBeVisible();
    expect(
      within(dialog).getByText("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
    ).toBeVisible();
  });

  it("keeps request and response protocol views isolated", async () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const request = screen.getByRole("heading", { name: "本机应答请求" }).closest("section");
    const response = screen.getByRole("heading", { name: "本机应答响应" }).closest("section");
    expect(request).not.toBeNull();
    expect(response).not.toBeNull();
    expect(await within(request!).findByTitle("协议包安全展示")).toBeVisible();
    expect(await within(response!).findByTitle("协议包安全展示")).toBeVisible();
  });

  it("uses the protocol view for a parsed request", async () => {
    const row = localRow();
    const detail = localDetail(row);
    if (detail.record.payload.kind !== "local_exchange") throw new Error("expected local exchange");
    detail.record.payload.capture.request_document = documentFixture;
    detail.record.payload.capture.request_display = {
      type: "untrusted_html",
      html: "<table><tbody><tr><th>MTI</th><td>0200</td></tr></tbody></table>",
    };
    renderDetail(row, detail);

    const request = screen.getByRole("heading", { name: "本机应答请求" }).closest("section");
    expect(await within(request!).findByTitle("协议包安全展示")).toBeVisible();
    expect(within(request!).queryByRole("table")).toBeNull();
  });

  it("shows matched downstream rules only inside the Response region", () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const request = screen.getByRole("heading", { name: "本机应答请求" }).closest("section");
    const response = screen.getByRole("heading", { name: "本机应答响应" }).closest("section");
    const ruleId = "99999999-9999-4999-8999-999999999999";
    expect(within(request!).queryByText(ruleId)).toBeNull();
    expect(within(response!).getByText(ruleId)).toBeVisible();
  });

  it("shows parsed request evidence when response generation fails", () => {
    const row = failedLocalRow();
    renderDetail(row, failedLocalDetail(row));

    expect(screen.getByText("响应生成失败")).toBeVisible();
    expect(screen.getByText("ENCODE_FAILED")).toBeVisible();
    expect(screen.getByRole("heading", { name: "已解析的应用请求" })).toBeVisible();
    expect(screen.getByText("未写出响应字节")).toBeVisible();
  });
});

describe("SocketCaptureDetail states and isolation", () => {
  it("shows a labelled loading state without stale detail", () => {
    renderDetail(relayRow(), undefined, { loading: true });

    expect(screen.getByLabelText("正在读取 Socket 抓包详情")).toBeVisible();
    expect(screen.queryByText("原始报文")).not.toBeInTheDocument();
  });

  it("shows a retry action for malformed detail", async () => {
    const user = userEvent.setup();
    const props = renderDetail(relayRow(), undefined, { malformed: true });

    expect(screen.getByText("详情数据校验失败")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(props.onRetry).toHaveBeenCalledTimes(1);
  });

  it("shows a transport detail error and retries it", async () => {
    const user = userEvent.setup();
    const props = renderDetail(relayRow(), undefined, {
      error: "detail unavailable",
    });

    expect(screen.getByText("详情读取失败")).toBeVisible();
    expect(screen.getByText("detail unavailable")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试" }));
    expect(props.onRetry).toHaveBeenCalledTimes(1);
  });

  it("closes through Escape and delegates payload release", async () => {
    const user = userEvent.setup();
    const props = renderDetail(relayRow(), relayDetail());

    await user.keyboard("{Escape}");
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("contains no HTTP-specific controls or fake upstream field", () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const dialog = screen.getByRole("dialog", { name: "Socket 抓包详情" });
    expect(dialog).not.toHaveTextContent(
      /Header|Cookie|Status|JSONPath|HTTP Body|HTTP 状态码|请求体|响应体|上游地址/i,
    );
    expect(within(dialog).queryByRole("tab", { name: /Header|Body|请求|响应/i })).toBeNull();
  });
});
