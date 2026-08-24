"""Contract tests for the external AU EFTEX transaction-trace verifier."""

from __future__ import annotations

import unittest

from Crypto.Cipher import DES3

from scripts.verify_au_eftex_trace import TraceFormatError, verify_trace_text


PUBLIC_BDK = bytes.fromhex("0123456789ABCDEFFEDCBA9876543210")
PUBLIC_KSN = bytes.fromhex("FFFF9876543210E00008")


def _header() -> bytes:
    return b"".join(
        [
            b"T",
            b"\xdf\x00\x01" + b"2",
            b"\xdf\x01\x08" + b"12345678",
            b"\xdf\x02\x06" + b"000001",
            b"\xdf\x03\x0a" + PUBLIC_KSN,
            b"B",
        ]
    )


def _message(message_type: str) -> bytes:
    bitmap = bytearray(8)
    bitmap[0] |= 0x20  # DE3
    return message_type.encode("ascii") + bytes(bitmap) + b"000000"


def _block(label: str, value: bytes, *, declared: int | None = None) -> str:
    length = len(value) if declared is None else declared
    return f"{label} Length({length})\n{value.hex().upper()}\n"


def _synthetic_trace() -> str:
    from au_eftex.codec import EftexCodec, pad_message
    from au_eftex.crypto import (
        derive_data_request_key,
        derive_data_response_key,
        derive_ipek,
        derive_transaction_key,
    )

    codec = EftexCodec(bdk=PUBLIC_BDK, context_key=bytes(32))
    ipek = derive_ipek(PUBLIC_BDK, PUBLIC_KSN)
    transaction_key = derive_transaction_key(ipek, PUBLIC_KSN)
    request_message = _message("1200")
    response_message = _message("1210")
    request_frame = codec.encrypt_frame("upstream", _header(), request_message)
    response_frame = codec.encrypt_frame("downstream", _header(), response_message)
    return "".join(
        [
            f"BDK:{PUBLIC_BDK.hex().upper()}\n",
            f"KSN:{PUBLIC_KSN.hex().upper()}\n",
            f"Initial Device Key: {ipek.hex().upper()} (000000)\n",
            f"DUKPT Key: {transaction_key.hex().upper()} (000000)\n",
            "Data Key Request: "
            f"{DES3.adjust_key_parity(derive_data_request_key(transaction_key)).hex().upper()}\n",
            "Data Key Response: "
            f"{DES3.adjust_key_parity(derive_data_response_key(transaction_key)).hex().upper()}\n",
            _block("Data to Cipher", pad_message(request_message)),
            _block("Encrypted Message (Including Header)", request_frame),
            _block("Encrypted Data", response_frame[39:]),
            _block("Decrypted Message", pad_message(response_message)),
        ]
    )


class TraceVerificationTests(unittest.TestCase):
    def test_replays_both_directions_and_preserves_exact_wire_bytes(self) -> None:
        result = verify_trace_text(_synthetic_trace())

        self.assertEqual(result.request_mti, "1200")
        self.assertEqual(result.response_mti, "1210")
        self.assertEqual(result.request_frame_bytes, 63)
        self.assertEqual(result.response_frame_bytes, 63)
        self.assertTrue(result.request_wire_round_trip)
        self.assertTrue(result.response_wire_round_trip)
        self.assertTrue(result.request_rpc_round_trip)
        self.assertTrue(result.response_rpc_round_trip)
        self.assertTrue(result.fragmented_frame_contract)

    def test_rejects_a_hex_block_whose_declared_length_is_false(self) -> None:
        trace = _synthetic_trace().replace(
            "Encrypted Message (Including Header) Length(63)",
            "Encrypted Message (Including Header) Length(64)",
        )

        with self.assertRaisesRegex(TraceFormatError, "declared 64 bytes"):
            verify_trace_text(trace)


if __name__ == "__main__":
    unittest.main()
