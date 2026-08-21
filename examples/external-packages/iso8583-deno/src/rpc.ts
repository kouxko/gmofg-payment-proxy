import { decodeBase64, encodeBase64 } from "./base64.ts";
import { decodeFrame, encodeFrame, FIELD_SPECS, frameBoundary, renderDocument } from "./iso8583.ts";
import type { DocumentWire, JsonRpcFailure, JsonRpcRequest, JsonRpcResponse } from "./types.ts";

const METHOD_NAMES = new Set([
  "hooks.upstream.split_frame",
  "hooks.upstream.decode_iso8583",
  "hooks.upstream.encode_iso8583",
  "document.upstream.render_message",
  "hooks.downstream.split_frame",
  "hooks.downstream.decode_iso8583",
  "hooks.downstream.encode_iso8583",
  "document.downstream.render_message",
]);

const schemaFields = [
  { name: "message_type", label: "MTI", type: "string" },
  ...FIELD_SPECS.map((field) => ({
    name: field.name,
    label: field.label,
    type: field.documentType,
  })),
];

export const REGISTRATION = {
  api: 1,
  package: {
    id: "iso8583-deno-ascii",
    name: "ISO 8583 Deno ASCII",
    version: "1.0.0",
    description: "Deno/TypeScript external package for a bounded ISO 8583:1987 ASCII profile",
  },
  document: {
    upstream: {
      schema: {
        id: "iso8583-deno-upstream",
        title: "ISO 8583:1987 ASCII Upstream",
        version: 1,
        fields: schemaFields,
      },
      display: "render_message",
    },
    downstream: {
      schema: {
        id: "iso8583-deno-downstream",
        title: "ISO 8583:1987 ASCII Downstream",
        version: 1,
        fields: schemaFields,
      },
      display: "render_message",
    },
  },
  hooks: {
    upstream: { frame: "split_frame", decode: "decode_iso8583", encode: "encode_iso8583" },
    downstream: { frame: "split_frame", decode: "decode_iso8583", encode: "encode_iso8583" },
  },
} as const;

export type RpcDispatcher = (request: JsonRpcRequest) => JsonRpcResponse;

/** Creates connection-local dispatch state; registration count must not leak across reconnects. */
export function createRpcDispatcher(): RpcDispatcher {
  let registered = false;
  return (request) => {
    try {
      if (request.method === "package.register") {
        if (registered) {
          return failure(
            request,
            -32001,
            "package.register may be called only once per connection",
          );
        }
        validateRegistrationRequest(request.params);
        registered = true;
        return success(request, REGISTRATION);
      }
      if (!METHOD_NAMES.has(request.method)) return failure(request, -32601, "method not found");
      const result = dispatchPackageMethod(request.method, request.params);
      return success(request, result);
    } catch (error) {
      return failure(request, -32002, safeErrorMessage(error));
    }
  };
}

function dispatchPackageMethod(method: string, params: unknown): unknown {
  if (method.endsWith(".split_frame")) {
    const value = strictObject(params, ["buffer_base64"], "frame params");
    return frameBoundary(decodeBase64(value.buffer_base64, "buffer_base64"));
  }
  if (method.endsWith(".decode_iso8583")) {
    const value = strictObject(params, ["frame_base64"], "decode params");
    return { document: decodeFrame(decodeBase64(value.frame_base64, "frame_base64")) };
  }
  if (method.endsWith(".encode_iso8583")) {
    const value = strictObject(params, ["document"], "encode params");
    const document = localResponseDefaults(method, asDocument(value.document));
    return { frame_base64: encodeBase64(encodeFrame(document)) };
  }
  const value = strictObject(params, ["document"], "display params");
  const document = localResponseDefaults(method, asDocument(value.document));
  return { html: renderDocument(document) };
}

/** LocalResponder starts its downstream document empty when no response rules are configured. */
function localResponseDefaults(method: string, document: DocumentWire): DocumentWire {
  const suppliesLocalResponse = method === "hooks.downstream.encode_iso8583" ||
    method === "document.downstream.render_message";
  if (!suppliesLocalResponse || document.message_type !== undefined) {
    return document;
  }
  return {
    ...document,
    message_type: { type: "string", value: "0210" },
    response_code: document.response_code ?? { type: "string", value: "00" },
  };
}

function validateRegistrationRequest(params: unknown): void {
  const value = strictObject(params, ["api"], "package.register params");
  if (value.api !== 1) throw new Error("only external package API 1 is supported");
}

function asDocument(value: unknown): DocumentWire {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("document must be an object");
  }
  return value as DocumentWire;
}

function strictObject(
  value: unknown,
  expectedKeys: readonly string[],
  name: string,
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${name} must be an object`);
  }
  const record = value as Record<string, unknown>;
  const actualKeys = Object.keys(record).sort();
  const expected = [...expectedKeys].sort();
  if (JSON.stringify(actualKeys) !== JSON.stringify(expected)) {
    throw new Error(`${name} contains missing or unknown fields`);
  }
  return record;
}

function success(request: JsonRpcRequest, result: unknown): JsonRpcResponse {
  return { jsonrpc: "2.0", id: request.id, result };
}

function failure(request: JsonRpcRequest, code: number, message: string): JsonRpcFailure {
  return { jsonrpc: "2.0", id: request.id, error: { code, message } };
}

function safeErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "external package processing failed";
}
