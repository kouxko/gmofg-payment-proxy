// @vitest-environment jsdom

/** Relay Frame 与 LocalExchange 详情的准确展示和 HTTP 隔离测试。 */

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
          decode_enabled: true,
          encode_enabled: true,
          origin: [0x30, 0x32],
          document: documentFixture,
          matched_rule_ids: row.matched_rule_ids,
          written: [0x30, 0x33],
          write_kind: "encoded",
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
          schema: row.schema,
          request_decode_enabled: false,
          response_encode_enabled: true,
          request_origin: [0x30],
          request_document: null,
          response_document: documentFixture,
          matched_downstream_rule_ids: row.matched_rule_ids,
          written_response: [0x31],
          response_write_kind: "encoded",
          response_display: { type: "untrusted_html", html: "<p>Local approved</p>" },
        },
      },
    },
  } satisfies SocketCaptureDetailViewModel;
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
  it("shows exact Relay direction, identities, bytes, Document and matched rules", () => {
    const row = relayRow();
    renderDetail(row, relayDetail(row));

    const dialog = screen.getByRole("dialog", { name: "Socket 抓包详情" });
    expect(within(dialog).getByText("Server → App")).toBeVisible();
    expect(within(dialog).getByText("iso8583@2.0.0")).toBeVisible();
    expect(within(dialog).getByText("payment-message v3")).toBeVisible();
    expect(within(dialog).getByText("9007199254740993")).toBeVisible();
    expect(
      within(dialog).getByText("66666666-6666-4666-8666-666666666666"),
    ).toBeVisible();
    expect(within(dialog).getByRole("region", { name: "Relay Origin" })).toBeVisible();
    expect(within(dialog).getByText("Encoded")).toBeVisible();
  });

  it("labels an encode-disabled Relay write as Raw Echo and defaults its fallback to Hex", () => {
    const row = relayRow();
    const detail = relayDetail(row);
    if (detail.record.payload.kind !== "relay_frame") throw new Error("relay fixture");
    detail.record.payload.capture.encode_enabled = false;
    detail.record.payload.capture.write_kind = "original";
    detail.record.payload.capture.display = {
      type: "hex_fallback",
      reason: "encode_disabled",
      diagnostic: null,
    };
    renderDetail(row, detail);

    expect(screen.getByText("Raw Echo")).toBeVisible();
    expect(screen.getByText("Encode 未启用，因此未调用 Display")).toBeVisible();
    expect(screen.getByRole("tab", { name: "Hex" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("shows a stable Display failure diagnostic without exposing script text", () => {
    const row = relayRow();
    const detail = relayDetail(row);
    if (detail.record.payload.kind !== "relay_frame") throw new Error("relay fixture");
    detail.record.payload.capture.display = {
      type: "hex_fallback",
      reason: "entry_point_failed",
      diagnostic: { code: "DISPLAY_FAILED", message: "协议展示失败" },
    };
    renderDetail(row, detail);

    expect(screen.getByText("Display 执行失败，默认显示 Hex")).toBeVisible();
    expect(screen.getByText("DISPLAY_FAILED")).toBeVisible();
    expect(screen.getByText(/协议展示失败/)).toBeVisible();
  });

  it("shows absent Relay Document and empty rule state explicitly", () => {
    const row = relayRow();
    const detail = relayDetail(row);
    if (detail.record.payload.kind !== "relay_frame") throw new Error("relay fixture");
    detail.record.payload.capture.decode_enabled = false;
    detail.record.payload.capture.document = null;
    detail.record.payload.capture.matched_rule_ids = [];
    renderDetail(row, detail);

    expect(screen.getByText("Decode 未启用，没有 Document。")).toBeVisible();
    expect(screen.getByText("无规则命中")).toBeVisible();
  });
});

describe("SocketCaptureDetail LocalExchange", () => {
  it("associates Request and Response under the same stable exchange ID", () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const dialog = screen.getByRole("dialog", { name: "Socket 抓包详情" });
    expect(within(dialog).getByRole("heading", { name: "LocalResponder Request" })).toBeVisible();
    expect(within(dialog).getByRole("heading", { name: "LocalResponder Response" })).toBeVisible();
    expect(
      within(dialog).getByText("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
    ).toBeVisible();
  });

  it("keeps an undecoded Request on built-in Hex and custom Display on Response only", async () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const request = screen.getByRole("heading", { name: "LocalResponder Request" }).closest("section");
    const response = screen.getByRole("heading", { name: "LocalResponder Response" }).closest("section");
    expect(request).not.toBeNull();
    expect(response).not.toBeNull();
    expect(within(request!).getByText("未启用（没有 Document）")).toBeVisible();
    expect(within(request!).getByRole("region", { name: "Local Request Hex" })).toBeVisible();
    expect(within(request!).queryByTitle("Socket 协议安全展示")).toBeNull();
    expect(await within(response!).findByTitle("Socket 协议安全展示")).toBeVisible();
    expect(within(response!).getByText("Encoded")).toBeVisible();
  });

  it("shows matched downstream rules only inside the Response region", () => {
    const row = localRow();
    renderDetail(row, localDetail(row));

    const request = screen.getByRole("heading", { name: "LocalResponder Request" }).closest("section");
    const response = screen.getByRole("heading", { name: "LocalResponder Response" }).closest("section");
    const ruleId = "99999999-9999-4999-8999-999999999999";
    expect(within(request!).queryByText(ruleId)).toBeNull();
    expect(within(response!).getByText(ruleId)).toBeVisible();
  });
});

describe("SocketCaptureDetail states and isolation", () => {
  it("shows a labelled loading state without stale detail", () => {
    renderDetail(relayRow(), undefined, { loading: true });

    expect(screen.getByLabelText("正在读取 Socket 抓包详情")).toBeVisible();
    expect(screen.queryByText("Origin 原始 Frame")).not.toBeInTheDocument();
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
