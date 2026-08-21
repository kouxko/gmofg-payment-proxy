import { decodeFrame, encodeFrame, frameBoundary, renderDocument } from "../src/iso8583.ts";
import { assert, assertEquals, assertThrows } from "./assert.ts";

const REQUEST_HEX =
  "0039303230303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932";

function hex(value: string): Uint8Array {
  return Uint8Array.from(value.match(/../g)?.map((byte) => Number.parseInt(byte, 16)) ?? []);
}

Deno.test("frame reports need_more for a split length header and payload", () => {
  assertEquals(frameBoundary(hex("00")), { status: "need_more" });
  assertEquals(frameBoundary(hex("0039303230")), { status: "need_more" });
});

Deno.test("frame consumes exactly one frame from a sticky buffer", () => {
  const frame = hex(REQUEST_HEX);
  const sticky = new Uint8Array(frame.length * 2);
  sticky.set(frame);
  sticky.set(frame, frame.length);
  assertEquals(frameBoundary(sticky), { status: "complete", consumed_bytes: frame.length });
});

Deno.test("decode and encode roundtrip the supported ASCII financial profile", () => {
  const frame = hex(REQUEST_HEX);
  const document = decodeFrame(frame);

  assertEquals(document.message_type, { type: "string", value: "0200" });
  assertEquals(document.processing_code, { type: "string", value: "000000" });
  assertEquals(document.amount, { type: "int", value: "1000" });
  assertEquals(document.stan, { type: "string", value: "123456" });
  assertEquals(document.terminal_id, { type: "string", value: "TERM0001" });
  assertEquals(encodeFrame(document), frame);
});

Deno.test("decode rejects trailing bytes and encode rejects unknown fields", () => {
  const frame = hex(REQUEST_HEX + "00");
  frame[0] = 0;
  frame[1] = frame.length - 2;
  assertThrows(() => decodeFrame(frame), "trailing bytes");
  assertThrows(
    () => encodeFrame({ unknown: { type: "string", value: "x" } }),
    "unknown Document field",
  );
});

Deno.test("display escapes untrusted field values and stays below 128 KiB", () => {
  const html = renderDocument({
    message_type: { type: "string", value: "<script>alert(1)</script>" },
  });
  assert(!html.includes("<script>"));
  assert(html.includes("&lt;script&gt;"));
  assert(new TextEncoder().encode(html).length <= 128 * 1024);
});
