from __future__ import annotations

import copy
import json
import unittest

from nuvei_tango_json.codec import TangoJsonCodec


def synthetic_frame(
    payload: dict[str, object] | None = None,
    *,
    control: bytes = b"\x01\x00\x01\x00",
    sequence: bytes = b"00000020",
) -> bytes:
    value = payload or {
            "AccptrAuthstnReq": {
                "Hdr": {"MsgFct": "FAUQ"},
                "Card": {
                    "PlainCardData": {
                        "PAN": "synthetic-pan",
                        "Track2": "synthetic-track-data",
                    }
                },
                "TxDtls": {"ICCRltdData": "sensitive-icc-data"},
                "SecurityTrailer": {"MAC": "secret-mac", "KeyId": "secret-key"},
        }
    }
    json_bytes = json.dumps(value, separators=(",", ":")).encode("utf-8")
    body = control + sequence + json_bytes
    return len(body).to_bytes(4, "big") + body


class TangoJsonCodecTests(unittest.TestCase):
    def setUp(self) -> None:
        self.codec = TangoJsonCodec(context_key=b"c" * 32)

    def test_split_frame_handles_fragmentation_and_sticky_buffer(self) -> None:
        frame = synthetic_frame()

        self.assertEqual(self.codec.frame("upstream", frame[:3]), {"status": "need_more"})
        self.assertEqual(
            self.codec.frame("upstream", frame[:-1]),
            {"status": "need_more"},
        )
        self.assertEqual(
            self.codec.frame("upstream", frame + b"next"),
            {"status": "complete", "consumed_bytes": len(frame)},
        )

    def test_decode_returns_read_only_masked_document_and_byte_exact_encode(self) -> None:
        frame = synthetic_frame()

        document = self.codec.decode("upstream", frame)

        self.assertEqual(
            document["frame_length"],
            {"type": "int", "value": str(len(frame) - 4)},
        )
        self.assertEqual(document["sequence"], {"type": "string", "value": "00000020"})
        self.assertEqual(
            document["message_type"],
            {"type": "string", "value": "AccptrAuthstnReq"},
        )
        preview = document["json_preview"]["value"]
        self.assertNotIn("synthetic-pan", preview)
        self.assertNotIn("synthetic-track-data", preview)
        self.assertNotIn("secret-mac", preview)
        self.assertNotIn("secret-key", preview)
        self.assertNotIn("sensitive-icc-data", preview)
        self.assertIn("[redacted]", preview)
        self.assertEqual(self.codec.encode("upstream", document), frame)

    def test_encode_rejects_every_public_document_change(self) -> None:
        document = self.codec.decode("upstream", synthetic_frame())
        for field in ("frame_length", "control_header", "sequence", "message_type", "json_preview"):
            with self.subTest(field=field):
                changed = copy.deepcopy(document)
                changed[field] = {"type": "string", "value": "changed"}
                with self.assertRaisesRegex(ValueError, "read-only document was modified"):
                    self.codec.encode("upstream", changed)

        added = copy.deepcopy(document)
        added["extra"] = {"type": "string", "value": "extra"}
        with self.assertRaisesRegex(ValueError, "read-only document was modified"):
            self.codec.encode("upstream", added)

        removed = copy.deepcopy(document)
        del removed["sequence"]
        with self.assertRaisesRegex(ValueError, "read-only document was modified"):
            self.codec.encode("upstream", removed)

    def test_encode_rejects_context_tampering_and_cross_direction_reuse(self) -> None:
        document = self.codec.decode("upstream", synthetic_frame())
        changed = copy.deepcopy(document)
        encoded = changed["encoding_context"]["value_base64"]
        changed["encoding_context"]["value_base64"] = encoded[:-2] + "AA"

        with self.assertRaisesRegex(ValueError, "encoding context"):
            self.codec.encode("upstream", changed)
        with self.assertRaisesRegex(ValueError, "encoding context"):
            self.codec.encode("downstream", document)

    def test_decode_rejects_invalid_wire_contracts(self) -> None:
        valid = synthetic_frame()
        invalid_frames = {
            "length": (len(valid) - 5).to_bytes(4, "big") + valid[4:],
            "sequence": synthetic_frame(sequence=b"ABCDEFGH"),
            "json": len(b"\x01\x00\x01\x00" + b"00000020" + b"{bad").to_bytes(4, "big")
            + b"\x01\x00\x01\x00"
            + b"00000020"
            + b"{bad",
        }
        for name, frame in invalid_frames.items():
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.codec.decode("upstream", frame)

    def test_decode_rejects_duplicate_json_keys_and_non_object_top_level(self) -> None:
        for json_bytes in (b'{"A":1,"A":2}', b"[]", b'{"A":NaN}'):
            body = b"\x01\x00\x01\x00" + b"00000020" + json_bytes
            frame = len(body).to_bytes(4, "big") + body
            with self.subTest(json_bytes=json_bytes), self.assertRaises(ValueError):
                self.codec.decode("upstream", frame)

    def test_display_is_html_escaped_and_contains_no_sensitive_values(self) -> None:
        frame = synthetic_frame({"<Message>": {"PAN": "synthetic-pan", "note": "<script>"}})
        document = self.codec.decode("downstream", frame)

        rendered = self.codec.display("downstream", document)

        self.assertIn("&lt;Message&gt;", rendered)
        self.assertIn("&lt;script&gt;", rendered)
        self.assertNotIn("synthetic-pan", rendered)
        self.assertNotIn("<script>", rendered)


if __name__ == "__main__":
    unittest.main()
