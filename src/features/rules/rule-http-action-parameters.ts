import type {
  HttpAction,
  RuleActionCapabilityViewModel,
  RuleActionKind,
  UnifiedAction,
} from "@/generated/rust-types";

export type HttpActionDraft = {
  kind: RuleActionKind | "";
  bodyText: string;
  milliseconds: string;
  minimumMilliseconds: string;
  maximumMilliseconds: string;
  jitterScope: "" | "before_message" | "per_chunk";
  bytesPerSecond: string;
  chunkBytes: string;
  availableMilliseconds: string;
  blockedMilliseconds: string;
  status: string;
  dropResponseMode: "" | "read_complete_response" | "close_after_request_write";
  invalidJsonBytes: string;
  delta: string;
  bytes: string;
  afterBytes: string;
};

export function newHttpActionDraft(kind: RuleActionKind | "" = ""): HttpActionDraft {
  return {
    kind,
    bodyText: "",
    milliseconds: "",
    minimumMilliseconds: "",
    maximumMilliseconds: "",
    jitterScope: "",
    bytesPerSecond: "",
    chunkBytes: "",
    availableMilliseconds: "",
    blockedMilliseconds: "",
    status: "",
    dropResponseMode: "",
    invalidJsonBytes: "",
    delta: "",
    bytes: "",
    afterBytes: "",
  };
}

export function httpActionDraft(action?: UnifiedAction): HttpActionDraft {
  if (!action || (action.source !== "http" && action.source !== "terminal")) {
    return newHttpActionDraft();
  }
  const [kind, value] = httpActionKindAndValue(action);
  const draft = newHttpActionDraft(kind);
  if (kind === "replace_body_text") return { ...draft, bodyText: String(value) };
  if (kind === "delay" && isRecord(value)) return { ...draft, milliseconds: numberText(value.milliseconds) };
  if (kind === "jitter" && isRecord(value)) {
    return {
      ...draft,
      minimumMilliseconds: numberText(value.minimum_milliseconds),
      maximumMilliseconds: numberText(value.maximum_milliseconds),
      jitterScope: value.scope === "BeforeMessage" ? "before_message" : value.scope === "PerChunk" ? "per_chunk" : "",
    };
  }
  if (kind === "throttle" && isRecord(value)) {
    return { ...draft, bytesPerSecond: numberText(value.bytes_per_second), chunkBytes: numberText(value.chunk_bytes) };
  }
  if (kind === "intermittent" && isRecord(value)) {
    return {
      ...draft,
      availableMilliseconds: numberText(value.available_milliseconds),
      blockedMilliseconds: numberText(value.blocked_milliseconds),
    };
  }
  if (kind === "custom_http_status" && isRecord(value)) return { ...draft, status: numberText(value.status) };
  if (isTimeoutKind(kind) && isRecord(value)) return { ...draft, milliseconds: numberText(value.milliseconds) };
  if (kind === "drop_upstream_response" && isRecord(value)) {
    return {
      ...draft,
      dropResponseMode: value.mode === "ReadCompleteResponse"
        ? "read_complete_response"
        : value.mode === "CloseAfterRequestWrite" ? "close_after_request_write" : "",
    };
  }
  if (kind === "invalid_json" && isRecord(value) && Array.isArray(value.body_bytes)) {
    const bytes = value.body_bytes.filter((item): item is number => typeof item === "number");
    return { ...draft, invalidJsonBytes: bytes.join(", ") };
  }
  if (kind === "incorrect_content_length" && isRecord(value)) return { ...draft, delta: numberText(value.delta) };
  if (kind === "truncate_response" && isRecord(value)) return { ...draft, bytes: numberText(value.bytes) };
  if (isDisconnectDuringWriteKind(kind) && isRecord(value)) return { ...draft, afterBytes: numberText(value.after_bytes) };
  return draft;
}

export function httpActionParametersJson(
  draft: HttpActionDraft,
  capability: RuleActionCapabilityViewModel | undefined,
): string | null | undefined {
  if (!draft.kind || capability?.kind !== draft.kind) return undefined;
  if (!capability.parameters_required) return null;

  if (draft.kind === "replace_body_text") return json({ text: draft.bodyText });
  if (draft.kind === "delay") return withUnsigned(draft.milliseconds, (milliseconds) => ({ milliseconds }));
  if (draft.kind === "jitter") {
    const minimum = unsignedInteger(draft.minimumMilliseconds);
    const maximum = unsignedInteger(draft.maximumMilliseconds);
    if (minimum == null || maximum == null || !draft.jitterScope) return undefined;
    return json({ minimum_milliseconds: minimum, maximum_milliseconds: maximum, scope: draft.jitterScope });
  }
  if (draft.kind === "throttle") {
    const bytesPerSecond = unsignedInteger(draft.bytesPerSecond);
    const chunkBytes = unsignedInteger(draft.chunkBytes);
    if (bytesPerSecond == null || chunkBytes == null || !capability.traffic_direction) return undefined;
    return json({ bytes_per_second: bytesPerSecond, chunk_bytes: chunkBytes, direction: capability.traffic_direction });
  }
  if (draft.kind === "intermittent") {
    const available = unsignedInteger(draft.availableMilliseconds);
    const blocked = unsignedInteger(draft.blockedMilliseconds);
    if (available == null || blocked == null || !capability.traffic_direction) return undefined;
    return json({ available_milliseconds: available, blocked_milliseconds: blocked, direction: capability.traffic_direction });
  }
  if (draft.kind === "custom_http_status") return withUnsigned(draft.status, (status) => ({ status }));
  if (isTimeoutKind(draft.kind)) return withUnsigned(draft.milliseconds, (milliseconds) => ({ milliseconds }));
  if (draft.kind === "drop_upstream_response") {
    return draft.dropResponseMode ? json({ mode: draft.dropResponseMode }) : undefined;
  }
  if (draft.kind === "invalid_json") {
    const bodyBytes = byteList(draft.invalidJsonBytes);
    return bodyBytes == null ? undefined : json({ body_bytes: bodyBytes });
  }
  if (draft.kind === "incorrect_content_length") {
    const delta = signedInteger(draft.delta);
    return delta == null ? undefined : json({ delta });
  }
  if (draft.kind === "truncate_response") return withUnsigned(draft.bytes, (bytes) => ({ bytes }));
  if (isDisconnectDuringWriteKind(draft.kind)) return withUnsigned(draft.afterBytes, (after_bytes) => ({ after_bytes }));
  return undefined;
}

function httpActionKindAndValue(
  action: Extract<UnifiedAction, { source: "http" | "terminal" }>,
): [RuleActionKind, unknown] {
  const value: HttpAction = action.source === "terminal" ? { Terminal: action.value } : action.value;
  if ("SetJsonField" in value) return ["set_json_field", value.SetJsonField];
  if ("ReplaceBodyText" in value) return ["replace_body_text", value.ReplaceBodyText];
  if ("SetHeader" in value) return ["set_header", value.SetHeader];
  if ("Delay" in value) return ["delay", value.Delay];
  if ("Jitter" in value) return ["jitter", value.Jitter];
  if ("Throttle" in value) return ["throttle", value.Throttle];
  if ("Intermittent" in value) return ["intermittent", value.Intermittent];
  if ("CustomHttpStatus" in value) return ["custom_http_status", value.CustomHttpStatus];
  const terminal = value.Terminal;
  if (terminal === "DisconnectBeforeUpstream") return ["disconnect_before_upstream", null];
  if ("UpstreamConnectTimeout" in terminal) return ["upstream_connect_timeout", terminal.UpstreamConnectTimeout];
  if ("UpstreamWriteTimeout" in terminal) return ["upstream_write_timeout", terminal.UpstreamWriteTimeout];
  if ("UpstreamReadTimeout" in terminal) return ["upstream_read_timeout", terminal.UpstreamReadTimeout];
  if ("DropUpstreamResponse" in terminal) return ["drop_upstream_response", terminal.DropUpstreamResponse];
  if ("MockResponse" in terminal) return ["mock_response", terminal.MockResponse];
  if ("InvalidJson" in terminal) return ["invalid_json", terminal.InvalidJson];
  if ("IncorrectContentLength" in terminal) return ["incorrect_content_length", terminal.IncorrectContentLength];
  if ("TruncateResponse" in terminal) return ["truncate_response", terminal.TruncateResponse];
  if ("DisconnectDuringUpstreamWrite" in terminal) return ["disconnect_during_upstream_write", terminal.DisconnectDuringUpstreamWrite];
  return ["disconnect_during_downstream_write", terminal.DisconnectDuringDownstreamWrite];
}

function isTimeoutKind(kind: RuleActionKind): boolean {
  return kind === "upstream_connect_timeout"
    || kind === "upstream_write_timeout"
    || kind === "upstream_read_timeout";
}

function isDisconnectDuringWriteKind(kind: RuleActionKind): boolean {
  return kind === "disconnect_during_upstream_write" || kind === "disconnect_during_downstream_write";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function numberText(value: unknown): string {
  return typeof value === "number" ? String(value) : "";
}

function unsignedInteger(value: string): number | undefined {
  if (!/^\d+$/.test(value.trim())) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
}

function signedInteger(value: string): number | undefined {
  if (!/^-?\d+$/.test(value.trim())) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : undefined;
}

function byteList(value: string): number[] | undefined {
  if (value.trim() === "") return [];
  const parts = value.split(",").map((item) => item.trim());
  if (parts.some((item) => !/^\d+$/.test(item))) return undefined;
  const bytes = parts.map(Number);
  return bytes.every((item) => Number.isInteger(item) && item >= 0 && item <= 255)
    ? bytes
    : undefined;
}

function withUnsigned<T>(value: string, build: (parsed: number) => T): string | undefined {
  const parsed = unsignedInteger(value);
  return parsed == null ? undefined : json(build(parsed));
}

function json(value: unknown): string {
  return JSON.stringify(value);
}
