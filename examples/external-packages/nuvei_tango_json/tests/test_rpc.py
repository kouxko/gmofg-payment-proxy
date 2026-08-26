from __future__ import annotations

import base64
import unittest

from nuvei_tango_json.codec import TangoJsonCodec
from nuvei_tango_json.rpc import REGISTRATION, create_rpc_dispatcher

from test_codec import synthetic_frame


class TangoJsonRpcTests(unittest.TestCase):
    def setUp(self) -> None:
        self.codec = TangoJsonCodec(context_key=b"r" * 32)
        self.dispatch = create_rpc_dispatcher(self.codec)

    def register(self) -> dict[str, object]:
        return self.dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )

    def test_registration_declares_read_only_nuvei_package(self) -> None:
        response = self.register()

        self.assertEqual(response["result"], REGISTRATION)
        self.assertEqual(REGISTRATION["package"]["id"], "nuvei-tango-json")
        self.assertIn("read-only", REGISTRATION["package"]["description"])
        for direction in ("upstream", "downstream"):
            self.assertEqual(
                REGISTRATION["hooks"][direction],
                {
                    "frame": "split_frame",
                    "decode": "decrypt_message",
                    "encode": "encrypt_message",
                },
            )

    def test_rpc_round_trip_preserves_synthetic_frame(self) -> None:
        self.register()
        frame = synthetic_frame()
        encoded = base64.b64encode(frame).decode("ascii")

        split = self.dispatch(
            {
                "jsonrpc": "2.0",
                "id": "split",
                "method": "hooks.upstream.split_frame",
                "params": {"buffer_base64": encoded},
            }
        )
        decoded = self.dispatch(
            {
                "jsonrpc": "2.0",
                "id": "decode",
                "method": "hooks.upstream.decrypt_message",
                "params": {"frame_base64": encoded},
            }
        )
        reencoded = self.dispatch(
            {
                "jsonrpc": "2.0",
                "id": "encode",
                "method": "hooks.upstream.encrypt_message",
                "params": {"document": decoded["result"]["document"]},
            }
        )

        self.assertEqual(split["result"], {"status": "complete", "consumed_bytes": len(frame)})
        self.assertEqual(reencoded["result"]["frame_base64"], encoded)

    def test_decode_serializes_int_as_canonical_decimal_string_for_proxy_wire(self) -> None:
        self.register()
        frame = synthetic_frame()

        decoded = self.dispatch(
            {
                "jsonrpc": "2.0",
                "id": "decode-wire-contract",
                "method": "hooks.upstream.decrypt_message",
                "params": {
                    "frame_base64": base64.b64encode(frame).decode("ascii"),
                },
            }
        )

        self.assertEqual(
            decoded["result"]["document"]["frame_length"],
            {"type": "int", "value": str(len(frame) - 4)},
        )

    def test_rpc_does_not_leak_codec_error_details(self) -> None:
        self.register()
        response = self.dispatch(
            {
                "jsonrpc": "2.0",
                "id": "bad",
                "method": "hooks.upstream.decrypt_message",
                "params": {"frame_base64": base64.b64encode(b"secret-invalid-frame").decode("ascii")},
            }
        )

        self.assertEqual(response["error"]["code"], -32002)
        self.assertEqual(response["error"]["message"], "external package processing failed [DECODE_FAILED]")
        self.assertNotIn("secret", response["error"]["message"])


if __name__ == "__main__":
    unittest.main()
