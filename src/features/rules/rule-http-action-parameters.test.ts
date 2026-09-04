import { describe, expect, it } from "vitest";
import type { RuleActionCapabilityViewModel, RuleActionKind } from "@/generated/rust-types";
import {
  httpActionDraft,
  httpActionParametersJson,
  newHttpActionDraft,
} from "./rule-http-action-parameters";

describe("HTTP action parameter drafts", () => {
  it.each([
    ["delay", { milliseconds: "70" }, { milliseconds: 70 }],
    ["upstream_connect_timeout", { milliseconds: "1000" }, { milliseconds: 1000 }],
    ["custom_http_status", { status: "503" }, { status: 503 }],
    ["incorrect_content_length", { delta: "-1" }, { delta: -1 }],
    ["truncate_response", { bytes: "12" }, { bytes: 12 }],
    ["disconnect_during_upstream_write", { afterBytes: "4" }, { after_bytes: 4 }],
  ] as const)("serializes %s into the existing Rust parameter contract", (kind, fields, expected) => {
    const draft = { ...newHttpActionDraft(kind), ...fields };
    expect(httpActionParametersJson(draft, capability(kind))).toBe(JSON.stringify(expected));
  });

  it("serializes structured weak-network parameters with the stage-owned direction", () => {
    const throttle = {
      ...newHttpActionDraft("throttle"),
      bytesPerSecond: "1024",
      chunkBytes: "4096",
    };
    expect(httpActionParametersJson(throttle, capability("throttle", "upstream"))).toBe(JSON.stringify({
      bytes_per_second: 1024,
      chunk_bytes: 4096,
      direction: "upstream",
    }));

    const intermittent = {
      ...newHttpActionDraft("intermittent"),
      availableMilliseconds: "500",
      blockedMilliseconds: "250",
    };
    expect(httpActionParametersJson(intermittent, capability("intermittent", "downstream"))).toBe(JSON.stringify({
      available_milliseconds: 500,
      blocked_milliseconds: 250,
      direction: "downstream",
    }));
  });

  it("serializes selects, bodies, and parameterless actions without raw JSON input", () => {
    expect(httpActionParametersJson({
      ...newHttpActionDraft("replace_body_text"),
      bodyText: "mock body",
    }, capability("replace_body_text"))).toBe(JSON.stringify({ text: "mock body" }));
    expect(httpActionParametersJson({
      ...newHttpActionDraft("jitter"),
      minimumMilliseconds: "1",
      maximumMilliseconds: "3",
      jitterScope: "per_chunk",
    }, capability("jitter"))).toBe(JSON.stringify({
      minimum_milliseconds: 1,
      maximum_milliseconds: 3,
      scope: "per_chunk",
    }));
    expect(httpActionParametersJson({
      ...newHttpActionDraft("drop_upstream_response"),
      dropResponseMode: "read_complete_response",
    }, capability("drop_upstream_response"))).toBe(JSON.stringify({ mode: "read_complete_response" }));
    expect(httpActionParametersJson({
      ...newHttpActionDraft("invalid_json"),
      invalidJsonBytes: "123, 105, 110, 118, 97, 108, 105, 100",
    }, capability("invalid_json"))).toBe(JSON.stringify({ body_bytes: [123, 105, 110, 118, 97, 108, 105, 100] }));
    expect(httpActionParametersJson(newHttpActionDraft("disconnect_before_upstream"), capability("disconnect_before_upstream", null, false))).toBeNull();
  });

  it("keeps required actions incomplete until every visible field is filled", () => {
    expect(httpActionParametersJson(newHttpActionDraft("delay"), capability("delay"))).toBeUndefined();
    expect(httpActionParametersJson({
      ...newHttpActionDraft("throttle"),
      bytesPerSecond: "1024",
      chunkBytes: "",
    }, capability("throttle", "upstream"))).toBeUndefined();
    expect(httpActionParametersJson({
      ...newHttpActionDraft("jitter"),
      minimumMilliseconds: "1",
      maximumMilliseconds: "2",
    }, capability("jitter"))).toBeUndefined();
  });

  it("reverse-fills persisted enum values and preserves existing invalid JSON bytes", () => {
    const jitter = httpActionDraft({
      source: "http",
      value: { Jitter: { minimum_milliseconds: 2, maximum_milliseconds: 8, scope: "BeforeMessage" } },
    });
    expect(jitter).toMatchObject({
      kind: "jitter",
      minimumMilliseconds: "2",
      maximumMilliseconds: "8",
      jitterScope: "before_message",
    });

    const invalid = httpActionDraft({
      source: "terminal",
      value: { InvalidJson: { body_bytes: [255, 0, 123] } },
    });
    expect(httpActionParametersJson(invalid, capability("invalid_json"))).toBe(JSON.stringify({ body_bytes: [255, 0, 123] }));
  });
});

function capability(
  kind: RuleActionKind,
  trafficDirection: "upstream" | "downstream" | null = null,
  parametersRequired = true,
): RuleActionCapabilityViewModel {
  return {
    kind,
    terminal: kind.includes("disconnect") || kind.includes("timeout") || kind === "drop_upstream_response",
    traffic_direction: trafficDirection,
    parameters_required: parametersRequired,
  };
}
