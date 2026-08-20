/** Socket 抓包分页、工作区选择和清空结果的 IPC 边界测试。 */

import { describe, expect, it } from "vitest";
import type { SocketCaptureRowViewModel } from "@/generated/rust-types";
import {
  validateOperationResult,
  validateSelectedWorkspace,
  validateSocketCapturePage,
} from "./socket-capture-model";

const workspaceId = "11111111-1111-4111-8111-111111111111";

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
    failure: null,
  };
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

function page() {
  return {
    rows: [relayRow(), localRow()],
    total: 2,
    page: 1,
    page_size: 50,
    total_pages: 1,
    empty_message: "暂无 Socket 抓包",
  };
}

describe("Socket capture page validation", () => {
  it("accepts exact Relay and Local row shapes", () => {
    expect(validateSocketCapturePage(page(), workspaceId)).toEqual(page());
  });

  it("accepts the real Rust empty-page contract with zero total pages", () => {
    const empty = {
      rows: [],
      total: 0,
      page: 1,
      page_size: 50,
      total_pages: 0,
      empty_message: "当前工作区还没有 Socket 抓包",
    };

    expect(validateSocketCapturePage(empty, workspaceId)).toEqual(empty);
  });

  it("accepts a real out-of-range page only when its row set is empty", () => {
    const outOfRange = {
      rows: [],
      total: 2,
      page: 3,
      page_size: 1,
      total_pages: 2,
      empty_message: "该页没有 Socket 抓包",
    };

    expect(validateSocketCapturePage(outOfRange, workspaceId)).toEqual(outOfRange);
  });

  it("rejects a partial in-range SQL page", () => {
    expect(
      validateSocketCapturePage({ ...page(), rows: [relayRow()] }, workspaceId),
    ).toBeUndefined();
  });

  it("rejects rows that contradict runtime record invariants", () => {
    const emptyOrigin = page();
    emptyOrigin.rows[0] = { ...relayRow(), origin_size_bytes: 0 };
    const splitSession = page();
    splitSession.rows[0] = { ...relayRow(), session_id: "different-session" };

    expect(validateSocketCapturePage(emptyOrigin, workspaceId)).toBeUndefined();
    expect(validateSocketCapturePage(splitSession, workspaceId)).toBeUndefined();
  });

  it("rejects an unknown page field", () => {
    expect(
      validateSocketCapturePage({ ...page(), http_status: 200 }, workspaceId),
    ).toBeUndefined();
  });

  it("rejects a Local row with a fabricated direction", () => {
    const value = page();
    value.rows[1] = { ...localRow(), direction: "upstream" };

    expect(validateSocketCapturePage(value, workspaceId)).toBeUndefined();
  });

  it("rejects duplicate capture IDs", () => {
    const value = page();
    value.rows[1] = { ...localRow(), capture_id: value.rows[0].capture_id };

    expect(validateSocketCapturePage(value, workspaceId)).toBeUndefined();
  });

  it("rejects more rows than the declared page size", () => {
    expect(
      validateSocketCapturePage({ ...page(), page_size: 1 }, workspaceId),
    ).toBeUndefined();
  });

  it("rejects an invalid package or Schema identity", () => {
    const invalidPackage = page();
    invalidPackage.rows[0] = {
      ...relayRow(),
      package: { id: "", version: "1.0.0" },
    };
    const invalidSchema = page();
    invalidSchema.rows[0] = {
      ...relayRow(),
      schema: { id: "payment-message", version: 0 },
    };

    expect(validateSocketCapturePage(invalidPackage, workspaceId)).toBeUndefined();
    expect(validateSocketCapturePage(invalidSchema, workspaceId)).toBeUndefined();
  });
});

describe("Socket capture supporting IPC validation", () => {
  const workspace = {
    id: workspaceId,
    name: "Socket workspace",
    revision: 3,
    listener_count: 2,
    enabled_listener_count: 1,
    selected: true,
  };

  it("returns the one exact selected workspace", () => {
    expect(
      validateSelectedWorkspace([
        { ...workspace, selected: false, id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd" },
        workspace,
      ]),
    ).toEqual(workspace);
  });

  it("rejects malformed or ambiguous workspace lists", () => {
    expect(validateSelectedWorkspace({ workspace })).toBeUndefined();
    expect(validateSelectedWorkspace([{ ...workspace, extra: true }])).toBeUndefined();
    expect(
      validateSelectedWorkspace([
        workspace,
        { ...workspace, id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee" },
      ]),
    ).toBeUndefined();
  });

  it("accepts a complete successful clear result", () => {
    const result = {
      success: true,
      cancelled: false,
      message: "cleared",
      ui_tone: "positive",
      entity_id: null,
      revision: null,
      requires_restart: false,
    } as const;

    expect(validateOperationResult(result)).toEqual(result);
  });

  it("rejects a partial, unknown-tone or negative-revision clear result", () => {
    expect(validateOperationResult({ success: true })).toBeUndefined();
    expect(
      validateOperationResult({
        success: true,
        cancelled: false,
        message: "cleared",
        ui_tone: "mystery",
        entity_id: null,
        revision: null,
        requires_restart: false,
      }),
    ).toBeUndefined();
    expect(
      validateOperationResult({
        success: true,
        cancelled: false,
        message: "cleared",
        ui_tone: "positive",
        entity_id: workspaceId,
        revision: -1,
        requires_restart: false,
      }),
    ).toBeUndefined();
  });
});
