/** Socket 抓包 IPC 边界和封闭联合的纯函数合约测试。 */

import { describe, expect, it } from "vitest";
import type {
  SocketCaptureDetailViewModel,
  SocketCaptureDocument,
  SocketCaptureRowViewModel,
} from "@/generated/rust-types";
import {
  defaultSocketCaptureQuery,
  packageLabel,
  schemaLabel,
  validateSocketCaptureDetail,
} from "./socket-capture-model";

const workspaceId = "11111111-1111-4111-8111-111111111111";

const documentFixture = {
  schema: {
    id: "payment-message",
    version: 7,
    title: "Payment message",
    fields: [
      { name: "mti", type: "string", label: "MTI" },
      { name: "amount", type: "int", label: "Amount" },
      { name: "approved", type: "bool", label: "Approved" },
      { name: "private_data", type: "blob", label: "Private data" },
    ],
  },
  values: [
    { type: "string", value: "0200" },
    { type: "int", value: "9223372036854775807" },
    { type: "bool", value: true },
    { type: "blob", value: [0, 127, 255] },
  ],
} satisfies SocketCaptureDocument;

function relayRow(): SocketCaptureRowViewModel {
  return {
    capture_id: "22222222-2222-4222-8222-222222222222",
    runtime_epoch: "33333333-3333-4333-8333-333333333333",
    session_id: "55555555-5555-4555-8555-555555555555",
    connection_id: "55555555-5555-4555-8555-555555555555",
    listener_id: "66666666-6666-4666-8666-666666666666",
    occurred_at: "2026-08-15T08:00:00Z",
    completed_at: "2026-08-15T08:00:01Z",
    kind: "relay_frame",
    direction: "upstream",
    package: { id: "iso8583", version: "1.2.3" },
    schema: { id: "payment-message", version: 7 },
    origin_size_bytes: 3,
    written_size_bytes: 2,
    logical_size_bytes: 512,
    matched_rule_ids: ["77777777-7777-4777-8777-777777777777"],
  };
}

function relayDetail(
  row: SocketCaptureRowViewModel = relayRow(),
): SocketCaptureDetailViewModel {
  return {
    record: {
      capture_id: row.capture_id,
      runtime_epoch: row.runtime_epoch,
      workspace_id: workspaceId,
      listener_id: row.listener_id,
      session_id: row.session_id,
      connection_id: row.connection_id,
      peer_address: "127.0.0.1:41000",
      occurred_at: row.occurred_at,
      completed_at: row.completed_at,
      payload: {
        kind: "relay_frame",
        capture: {
          direction: "upstream",
          package: row.package,
          schema: row.schema,
          decode_enabled: true,
          encode_enabled: true,
          origin: [0x30, 0x32, 0x30],
          document: documentFixture,
          matched_rule_ids: row.matched_rule_ids,
          written: [0x31, 0x32],
          write_kind: "encoded",
          display: {
            type: "untrusted_html",
            html: "<p>ISO 8583</p>",
          },
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
    origin_size_bytes: 2,
    written_size_bytes: 3,
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
      workspace_id: workspaceId,
      listener_id: row.listener_id,
      session_id: row.session_id,
      connection_id: row.connection_id,
      peer_address: "127.0.0.1:42000",
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
          request_origin: [0x30, 0x32],
          request_document: null,
          request_display: null,
          response_document: documentFixture,
          matched_downstream_rule_ids: row.matched_rule_ids,
          written_response: [0x30, 0x32, 0x31],
          response_write_kind: "encoded",
          response_display: {
            type: "untrusted_html",
            html: "<p>Approved</p>",
          },
        },
      },
    },
  } satisfies SocketCaptureDetailViewModel;
}

describe("Socket capture query model", () => {
  it("contains only Socket query dimensions and starts from the first page", () => {
    const query = defaultSocketCaptureQuery(workspaceId);

    expect(query).toEqual({
      workspace_id: workspaceId,
      listener_id: null,
      session_id: null,
      connection_id: null,
      package: null,
      direction: null,
      kind: null,
      occurred_from: null,
      occurred_to: null,
      sort: "occurred_at",
      direction_sort: "desc",
      page: { page: 1, page_size: 50 },
    });
    expect(Object.keys(query).join(" ")).not.toMatch(
      /header|cookie|status|json|body|method|path/i,
    );
  });

  it("formats exact package and schema identities without dropping versions", () => {
    const row = relayRow();

    expect(packageLabel(row.package)).toBe("iso8583@1.2.3");
    expect(schemaLabel(row.schema)).toBe("payment-message v7");
  });
});

describe("Socket capture detail validation", () => {
  it("accepts a Relay detail with all four Document value types", () => {
    const row = relayRow();

    const validated = validateSocketCaptureDetail(
      relayDetail(row),
      row,
      workspaceId,
    );

    expect(validated).toBeDefined();
    if (validated?.record.payload.kind !== "relay_frame") {
      throw new Error("expected relay fixture");
    }
    expect(validated.record.payload.capture.document?.values).toEqual(
      documentFixture.values,
    );
    expect(validated.record.payload.capture.document?.values[1]).toEqual({
      type: "int",
      value: "9223372036854775807",
    });
  });

  it("accepts a Local exchange whose undecoded request has no fake Document", () => {
    const row = localRow();

    const validated = validateSocketCaptureDetail(
      localDetail(row),
      row,
      workspaceId,
    );

    expect(validated?.record.payload.kind).toBe("local_exchange");
    if (validated?.record.payload.kind !== "local_exchange") {
      throw new Error("expected local fixture");
    }
    expect(validated.record.payload.capture.request_document).toBeNull();
    expect(validated.record.payload.capture.response_document).toEqual(
      documentFixture,
    );
  });

  it("rejects a payload branch that disagrees with the selected row kind", () => {
    const row = localRow();
    const detail = relayDetail({
      ...row,
      kind: "relay_frame",
      direction: "upstream",
      origin_size_bytes: 3,
      written_size_bytes: 2,
    });

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects a detail from another workspace or selected capture", () => {
    const row = relayRow();
    const detail = relayDetail(row);

    expect(
      validateSocketCaptureDetail(detail, row, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
    ).toBeUndefined();
    expect(
      validateSocketCaptureDetail(
        detail,
        { ...row, capture_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc" },
        workspaceId,
      ),
    ).toBeUndefined();
  });

  it("rejects an invalid i64 text instead of coercing it through JavaScript number", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame" || !detail.record.payload.capture.document) {
      throw new Error("expected decoded relay fixture");
    }
    detail.record.payload.capture.document.values[1] = {
      type: "int",
      value: "01",
    };

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("accepts the exact signed i64 minimum as decimal text", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame" || !detail.record.payload.capture.document) {
      throw new Error("expected decoded relay fixture");
    }
    detail.record.payload.capture.document.values[1] = {
      type: "int",
      value: "-9223372036854775808",
    };

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeDefined();
  });

  it("rejects decimal text outside the signed i64 range", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame" || !detail.record.payload.capture.document) {
      throw new Error("expected decoded relay fixture");
    }
    detail.record.payload.capture.document.values[1] = {
      type: "int",
      value: "9223372036854775808",
    };

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects an out-of-range Blob byte", () => {
    const row = localRow();
    const detail = structuredClone(localDetail(row));
    if (detail.record.payload.kind !== "local_exchange") {
      throw new Error("expected local fixture");
    }
    detail.record.payload.capture.response_document.values[3] = {
      type: "blob",
      value: [256],
    };

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects Relay encode evidence whose write kind contradicts the enabled flag", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame") {
      throw new Error("expected relay fixture");
    }
    detail.record.payload.capture.encode_enabled = false;

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects encode-enabled evidence that claims the encode-disabled Display reason", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame") {
      throw new Error("expected relay fixture");
    }
    detail.record.payload.capture.display = {
      type: "hex_fallback",
      reason: "encode_disabled",
      diagnostic: null,
    };

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects encode-disabled evidence with a fabricated Display diagnostic", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame") {
      throw new Error("expected relay fixture");
    }
    detail.record.payload.capture.encode_enabled = false;
    detail.record.payload.capture.write_kind = "original";
    detail.record.payload.capture.written = [...detail.record.payload.capture.origin];
    row.written_size_bytes = row.origin_size_bytes;
    detail.record.payload.capture.display = {
      type: "hex_fallback",
      reason: "encode_disabled",
      diagnostic: { code: "IMPOSSIBLE", message: "must be absent" },
    };

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects a Document Schema with duplicate field names", () => {
    const row = relayRow();
    const detail = structuredClone(relayDetail(row));
    if (detail.record.payload.kind !== "relay_frame" || !detail.record.payload.capture.document) {
      throw new Error("expected decoded relay fixture");
    }
    detail.record.payload.capture.document.schema.fields[1].name = "mti";

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });

  it("rejects Local response evidence whose write kind contradicts the enabled flag", () => {
    const row = localRow();
    const detail = structuredClone(localDetail(row));
    if (detail.record.payload.kind !== "local_exchange") {
      throw new Error("expected local fixture");
    }
    detail.record.payload.capture.response_encode_enabled = false;

    expect(validateSocketCaptureDetail(detail, row, workspaceId)).toBeUndefined();
  });
});
