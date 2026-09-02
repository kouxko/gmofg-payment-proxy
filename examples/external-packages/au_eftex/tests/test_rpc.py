from __future__ import annotations

import base64
import unittest

from au_eftex.rpc import REGISTRATION, create_rpc_dispatcher


class FakeCodec:
    def frame(self, direction: str, buffer: bytes) -> dict[str, object]:
        return {"status": "complete", "consumed_bytes": len(buffer)}

    def decode(self, direction: str, frame: bytes) -> dict[str, object]:
        return {
            "message": {
                "type": "blob",
                "value_base64": base64.b64encode(frame).decode("ascii"),
            }
        }

    def encode(self, direction: str, document: dict[str, object]) -> bytes:
        return direction.encode("ascii")

    def display(self, direction: str, document: dict[str, object]) -> str:
        return f"<p>{direction}</p>"


class ExplodingCodec(FakeCodec):
    def decode(self, direction: str, frame: bytes) -> dict[str, object]:
        raise RuntimeError("BDK=do-not-leak")


class InvalidLengthPrefixCodec(FakeCodec):
    def frame(self, direction: str, buffer: bytes) -> dict[str, object]:
        raise ValueError("AU EFTEX length prefix does not match the complete frame")


class WrongDataKeyDirectionCodec(FakeCodec):
    def frame(self, direction: str, buffer: bytes) -> dict[str, object]:
        raise ValueError("AU EFTEX data key direction does not match the hook direction")


class RpcDispatcherTests(unittest.TestCase):
    def test_rejects_crypto_calls_before_proxy_registration(self) -> None:
        response = create_rpc_dispatcher(FakeCodec())(
            {
                "jsonrpc": "2.0",
                "id": "early-call",
                "method": "hooks.upstream.decrypt_message",
                "params": {"frame_base64": "AA=="},
            }
        )

        self.assertEqual(response["error"]["code"], -32003)
        self.assertEqual(response["error"]["message"], "package.register must complete first")

    def test_package_register_returns_strict_manifest_once_per_connection(self) -> None:
        dispatch = create_rpc_dispatcher(FakeCodec())

        first = dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register-1",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        second = dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register-2",
                "method": "package.register",
                "params": {"api": 1},
            }
        )

        self.assertEqual(first, {"jsonrpc": "2.0", "id": "register-1", "result": REGISTRATION})
        self.assertEqual(REGISTRATION["package"]["id"], "au-eftex")
        self.assertEqual(REGISTRATION["package"]["name"], "AU EFTEX")
        self.assertEqual(REGISTRATION["package"]["version"], "1.1.0")
        self.assertEqual(
            second,
            {
                "jsonrpc": "2.0",
                "id": "register-2",
                "error": {
                    "code": -32001,
                    "message": "package.register may be called only once per connection",
                },
            },
        )

    def test_registration_declares_parsed_iso8583_fields_and_exact_methods(self) -> None:
        for direction in ("upstream", "downstream"):
            document = REGISTRATION["document"][direction]
            fields = document["schema"]["fields"]
            self.assertEqual(fields[0]["name"], "message_type")
            self.assertIn(
                {"name": "amount", "label": "DE4 Amount", "type": "int"},
                fields,
            )
            self.assertIn(
                {
                    "name": "message_authentication_code",
                    "label": "DE64 Message Authentication Code",
                    "type": "blob",
                },
                fields,
            )
            self.assertIn(
                {
                    "name": "icc_data",
                    "label": "DE55 ICC System Related Data",
                    "type": "blob",
                },
                fields,
            )
            self.assertIn(
                {
                    "name": "message_authentication_code_extended",
                    "label": "DE128 Message Authentication Code",
                    "type": "blob",
                },
                fields,
            )
            self.assertEqual(fields[-1]["name"], "encoding_context")
            self.assertEqual(document["display"], "render_message")
            self.assertEqual(
                REGISTRATION["hooks"][direction],
                {
                    "frame": "split_frame",
                    "decode": "decrypt_message",
                    "encode": "encrypt_message",
                },
            )

    def test_dispatches_all_fully_qualified_methods(self) -> None:
        dispatch = create_rpc_dispatcher(FakeCodec())
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        encoded = base64.b64encode(b"abc").decode("ascii")

        for direction in ("upstream", "downstream"):
            self.assertEqual(
                dispatch(
                    {
                        "jsonrpc": "2.0",
                        "id": f"{direction}-frame",
                        "method": f"hooks.{direction}.split_frame",
                        "params": {"buffer_base64": encoded},
                    }
                )["result"],
                {"status": "complete", "consumed_bytes": 3},
            )
            self.assertEqual(
                dispatch(
                    {
                        "jsonrpc": "2.0",
                        "id": f"{direction}-decode",
                        "method": f"hooks.{direction}.decrypt_message",
                        "params": {"frame_base64": encoded},
                    }
                )["result"],
                {
                    "document": {
                        "message": {"type": "blob", "value_base64": encoded},
                    }
                },
            )
            self.assertEqual(
                dispatch(
                    {
                        "jsonrpc": "2.0",
                        "id": f"{direction}-encode",
                        "method": f"hooks.{direction}.encrypt_message",
                        "params": {"document": {}},
                    }
                )["result"],
                {"frame_base64": base64.b64encode(direction.encode("ascii")).decode("ascii")},
            )
            self.assertEqual(
                dispatch(
                    {
                        "jsonrpc": "2.0",
                        "id": f"{direction}-display",
                        "method": f"document.{direction}.render_message",
                        "params": {"document": {}},
                    }
                )["result"],
                {"html": f"<p>{direction}</p>"},
            )

    def test_rejects_noncanonical_base64_and_unknown_params(self) -> None:
        dispatch = create_rpc_dispatcher(FakeCodec())
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )

        for params in ({"frame_base64": "YWJj\n"}, {"frame_base64": "YWJj", "extra": 1}):
            response = dispatch(
                {
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": "hooks.upstream.decrypt_message",
                    "params": params,
                }
            )
            self.assertEqual(response["error"]["code"], -32002)

    def test_codec_exception_does_not_expose_secret_material(self) -> None:
        dispatch = create_rpc_dispatcher(ExplodingCodec())
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )
        response = dispatch(
            {
                "jsonrpc": "2.0",
                "id": 9,
                "method": "hooks.upstream.decrypt_message",
                "params": {"frame_base64": "AA=="},
            }
        )

        self.assertEqual(
            response["error"]["message"],
            "external package processing failed [DECODE_FAILED]",
        )
        self.assertNotIn("BDK", str(response))

    def test_length_prefix_failure_has_a_stable_safe_error_code(self) -> None:
        dispatch = create_rpc_dispatcher(InvalidLengthPrefixCodec())
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )

        response = dispatch(
            {
                "jsonrpc": "2.0",
                "id": "bad-length",
                "method": "hooks.upstream.split_frame",
                "params": {"buffer_base64": "AA=="},
            }
        )

        self.assertEqual(
            response["error"]["message"],
            "external package processing failed [LENGTH_PREFIX_INVALID]",
        )

    def test_data_key_direction_failure_has_a_stable_safe_error_code(self) -> None:
        dispatch = create_rpc_dispatcher(WrongDataKeyDirectionCodec())
        dispatch(
            {
                "jsonrpc": "2.0",
                "id": "register",
                "method": "package.register",
                "params": {"api": 1},
            }
        )

        response = dispatch(
            {
                "jsonrpc": "2.0",
                "id": "wrong-direction",
                "method": "hooks.upstream.split_frame",
                "params": {"buffer_base64": "AA=="},
            }
        )

        self.assertEqual(
            response["error"]["message"],
            "external package processing failed [DATA_KEY_DIRECTION_MISMATCH]",
        )


if __name__ == "__main__":
    unittest.main()
