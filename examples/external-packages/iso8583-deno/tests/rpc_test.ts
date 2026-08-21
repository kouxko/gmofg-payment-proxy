import { createRpcDispatcher, REGISTRATION } from "../src/rpc.ts";
import { decodeBase64, encodeBase64 } from "../src/base64.ts";
import { assertEquals } from "./assert.ts";

const REQUEST_HEX =
  "0039303230303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932";

function hex(value: string): Uint8Array {
  return Uint8Array.from(value.match(/../g)?.map((byte) => Number.parseInt(byte, 16)) ?? []);
}

Deno.test("package.register returns the strict production registration once", async () => {
  const dispatch = createRpcDispatcher();
  const response = await dispatch({
    jsonrpc: "2.0",
    id: "register-1",
    method: "package.register",
    params: { api: 1 },
  });

  assertEquals(response, { jsonrpc: "2.0", id: "register-1", result: REGISTRATION });
  assertEquals(
    await dispatch({
      jsonrpc: "2.0",
      id: "register-2",
      method: "package.register",
      params: { api: 1 },
    }),
    {
      jsonrpc: "2.0",
      id: "register-2",
      error: { code: -32001, message: "package.register may be called only once per connection" },
    },
  );
});

Deno.test("dispatcher exposes exact upstream and downstream qualified method names", async () => {
  for (const direction of ["upstream", "downstream"] as const) {
    const dispatch = createRpcDispatcher();
    const frame = await dispatch({
      jsonrpc: "2.0",
      id: `${direction}-frame`,
      method: `hooks.${direction}.split_frame`,
      params: { buffer_base64: "AA==" },
    });
    assertEquals(frame, {
      jsonrpc: "2.0",
      id: `${direction}-frame`,
      result: { status: "need_more" },
    });

    const sticky = new Uint8Array(hex(REQUEST_HEX).length * 2);
    sticky.set(hex(REQUEST_HEX));
    sticky.set(hex(REQUEST_HEX), hex(REQUEST_HEX).length);
    assertEquals(
      await dispatch({
        jsonrpc: "2.0",
        id: `${direction}-sticky`,
        method: `hooks.${direction}.split_frame`,
        params: { buffer_base64: encodeBase64(sticky) },
      }),
      {
        jsonrpc: "2.0",
        id: `${direction}-sticky`,
        result: { status: "complete", consumed_bytes: hex(REQUEST_HEX).length },
      },
    );
  }
});

Deno.test("qualified decode, encode and display methods form a safe roundtrip", async () => {
  const dispatch = createRpcDispatcher();
  const frame = hex(REQUEST_HEX);
  const decoded = await dispatch({
    jsonrpc: "2.0",
    id: "decode",
    method: "hooks.upstream.decode_iso8583",
    params: { frame_base64: encodeBase64(frame) },
  });
  if (!("result" in decoded)) throw new Error("decode failed");
  const document = (decoded.result as { document: unknown }).document;

  const encoded = await dispatch({
    jsonrpc: "2.0",
    id: "encode",
    method: "hooks.downstream.encode_iso8583",
    params: { document },
  });
  if (!("result" in encoded)) throw new Error("encode failed");
  assertEquals(
    decodeBase64((encoded.result as { frame_base64: string }).frame_base64, "frame"),
    frame,
  );

  const displayed = await dispatch({
    jsonrpc: "2.0",
    id: "display",
    method: "document.downstream.render_message",
    params: { document },
  });
  if (!("result" in displayed)) throw new Error("display failed");
  assertEquals(
    (displayed.result as { html: string }).html.includes("ISO 8583:1987 Message"),
    true,
  );
});

Deno.test("downstream encode supplies a minimal response for LocalResponder without rules", async () => {
  const dispatch = createRpcDispatcher();

  const encoded = await dispatch({
    jsonrpc: "2.0",
    id: "local-response",
    method: "hooks.downstream.encode_iso8583",
    params: { document: {} },
  });

  assertEquals(encoded, {
    jsonrpc: "2.0",
    id: "local-response",
    result: { frame_base64: encodeBase64(hex("000e3032313000000000020000003030")) },
  });
});

Deno.test("downstream display renders the same minimal LocalResponder response", async () => {
  const dispatch = createRpcDispatcher();

  const displayed = await dispatch({
    jsonrpc: "2.0",
    id: "local-response-display",
    method: "document.downstream.render_message",
    params: { document: {} },
  });

  if (!("result" in displayed)) throw new Error("display failed");
  const html = (displayed.result as { html: string }).html;
  assertEquals(html.includes(">0210<"), true);
  assertEquals(html.includes("DE39 Response Code"), true);
  assertEquals(html.includes(">00<"), true);
});

Deno.test("dispatcher preserves numeric and string correlation IDs", async () => {
  const dispatch = createRpcDispatcher();
  const first = await dispatch({
    jsonrpc: "2.0",
    id: 42,
    method: "hooks.upstream.split_frame",
    params: { buffer_base64: "AA==" },
  });
  assertEquals(first.id, 42);
});
