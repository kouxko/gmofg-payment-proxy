from __future__ import annotations

import base64
import unittest

from au_eftex.codec import EftexCodec, _data_iv, pad_message, unpad_message
from au_eftex.iso8583 import encode_message


PUBLIC_BDK = bytes.fromhex("0123456789ABCDEFFEDCBA9876543210")
PUBLIC_KSN = bytes.fromhex("FFFF9876543210E00008")


def build_header(ksn: bytes = PUBLIC_KSN) -> bytes:
    return b"".join(
        [
            b"T",
            b"\xdf\x00\x01" + b"2",
            b"\xdf\x01\x08" + b"12345678",
            b"\xdf\x02\x06" + b"000001",
            b"\xdf\x03\x0a" + ksn,
            b"B",
        ]
    )


def minimal_message() -> bytes:
    bitmap = bytearray(8)
    bitmap[0] |= 0x20  # DE3
    bitmap[7] |= 0x01  # DE64
    return b"1200" + bytes(bitmap) + b"000000" + bytes.fromhex("0102030405060708")


def minimal_message_without_mac() -> bytes:
    bitmap = bytearray(8)
    bitmap[0] |= 0x20  # DE3
    return b"1210" + bytes(bitmap) + b"000000"


class EftexPaddingTests(unittest.TestCase):
    def test_padding_uses_ff_fill_and_a_fill_count_byte(self) -> None:
        self.assertEqual(pad_message(b"ABCDEF"), b"ABCDEF\xff\x01")
        self.assertEqual(pad_message(b"ABCDEFG"), b"ABCDEFG\x00")

    def test_aligned_message_receives_a_full_padding_block(self) -> None:
        padded = pad_message(b"ABCDEFGH")
        self.assertEqual(padded, b"ABCDEFGH" + b"\xff" * 7 + b"\x07")

    def test_unpadding_rejects_invalid_fill_bytes(self) -> None:
        with self.assertRaisesRegex(ValueError, "padding"):
            unpad_message(b"ABCDEF\x00\x01")


class EftexCodecTests(unittest.TestCase):
    def setUp(self) -> None:
        self.codec = EftexCodec(bdk=PUBLIC_BDK, context_key=bytes(32))
        self.header = build_header()
        self.message = minimal_message()

    def test_data_iv_is_derived_from_the_h01_stan(self) -> None:
        self.assertEqual(
            _data_iv(self.header),
            bytes.fromhex("0123456789ABCDEE"),
        )

    def test_encrypt_then_decrypt_preserves_the_complete_message(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)

        decoded = self.codec.decrypt_frame("upstream", frame)

        self.assertEqual(decoded.header, self.header)
        self.assertEqual(decoded.message, self.message)

    def test_public_synthetic_complete_wire_golden_vectors(self) -> None:
        # Ciphertexts were generated independently with OpenSSL 3.6.3
        # des-ede3-ofb using public ANSI DUKPT material and synthetic ISO data.
        upstream = bytes.fromhex(
            "54DF000132DF01083132333435363738DF0206303030303031"
            "DF030AFFFF9876543210E0000842"
            "7B758DDA6A29D38B8020B31687B21D636DBC15E6F3A17CDEE8A868124D4C8F84"
        )
        downstream = bytes.fromhex(
            "54DF000132DF01083132333435363738DF0206303030303031"
            "DF030AFFFF9876543210E0000842"
            "47737E0317A4310697A84E728F754C84798309EF10EDD18E"
        )

        self.assertEqual(
            self.codec.encrypt_frame("upstream", self.header, minimal_message()),
            upstream,
        )
        self.assertEqual(
            self.codec.encrypt_frame(
                "downstream",
                self.header,
                minimal_message_without_mac(),
            ),
            downstream,
        )
        self.assertEqual(
            self.codec.encode("upstream", self.codec.decode("upstream", upstream)),
            upstream,
        )
        self.assertEqual(
            self.codec.encode("downstream", self.codec.decode("downstream", downstream)),
            downstream,
        )

    def test_downstream_uses_a_distinct_data_key_variant(self) -> None:
        upstream = self.codec.encrypt_frame("upstream", self.header, self.message)
        downstream = self.codec.encrypt_frame("downstream", self.header, self.message)

        self.assertNotEqual(upstream[39:], downstream[39:])
        self.assertEqual(self.codec.decrypt_frame("downstream", downstream).message, self.message)

    def test_frame_boundary_handles_partial_and_sticky_buffers(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)

        self.assertEqual(self.codec.frame_boundary("upstream", frame[:-1]), {"status": "need_more"})
        self.assertEqual(
            self.codec.frame_boundary("upstream", frame + frame),
            {"status": "complete", "consumed_bytes": len(frame)},
        )

    def test_frame_boundary_identifies_the_opposite_data_key_direction(self) -> None:
        frame = self.codec.encrypt_frame("downstream", self.header, self.message)

        with self.assertRaisesRegex(ValueError, "data key direction"):
            self.codec.frame_boundary("upstream", frame)

    def test_two_byte_length_prefix_excluding_itself_is_preserved(self) -> None:
        body = self.codec.encrypt_frame("upstream", self.header, self.message)
        frame = len(body).to_bytes(2, "big") + body

        self.assertEqual(self.codec.frame_boundary("upstream", frame[:-1]), {"status": "need_more"})
        self.assertEqual(
            self.codec.frame_boundary("upstream", frame + frame),
            {"status": "complete", "consumed_bytes": len(frame)},
        )
        document = self.codec.decode("upstream", frame)
        self.assertEqual(self.codec.encode("upstream", document), frame)

    def test_two_byte_length_prefix_including_itself_is_preserved(self) -> None:
        body = self.codec.encrypt_frame("downstream", self.header, self.message)
        frame = (len(body) + 2).to_bytes(2, "big") + body

        document = self.codec.decode("downstream", frame)

        self.assertEqual(self.codec.encode("downstream", document), frame)

    def test_two_byte_length_prefix_must_match_the_complete_frame(self) -> None:
        body = self.codec.encrypt_frame("upstream", self.header, self.message)
        frame = (len(body) + 1).to_bytes(2, "big") + body

        with self.assertRaisesRegex(ValueError, "length prefix"):
            self.codec.frame_boundary("upstream", frame)

    def test_two_byte_length_prefix_frames_without_decrypting_payload(self) -> None:
        body = self.header + bytes(272)
        frame = len(body).to_bytes(2, "big") + body

        self.assertEqual(
            self.codec.frame_boundary("upstream", frame + frame),
            {"status": "complete", "consumed_bytes": len(frame)},
        )

    def test_document_round_trip_exposes_parsed_iso8583_fields(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)

        document = self.codec.decode("upstream", frame)
        encoded = self.codec.encode("upstream", document)

        self.assertEqual(encoded, frame)
        self.assertEqual(document["message_type"], {"type": "string", "value": "1200"})
        self.assertEqual(document["processing_code"], {"type": "string", "value": "000000"})
        self.assertEqual(
            document["message_authentication_code"],
            {"type": "blob", "value_base64": base64.b64encode(bytes.fromhex("0102030405060708")).decode()},
        )
        self.assertEqual(document["encoding_context"]["type"], "blob")

    def test_encode_rejects_authenticated_field_changes_without_a_new_mac(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)
        document = self.codec.decode("upstream", frame)
        document["processing_code"] = {"type": "string", "value": "990000"}

        with self.assertRaisesRegex(ValueError, "MAC"):
            self.codec.encode("upstream", document)

    def test_encode_rejects_a_field_change_even_when_the_caller_replaces_the_mac(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)
        document = self.codec.decode("upstream", frame)
        document["processing_code"] = {"type": "string", "value": "990000"}
        document["message_authentication_code"] = {
            "type": "blob",
            "value_base64": base64.b64encode(b"NEWMAC!!").decode(),
        }

        with self.assertRaisesRegex(ValueError, "MAC"):
            self.codec.encode("upstream", document)

    def test_encode_rejects_adding_a_mac_while_changing_a_message_without_one(self) -> None:
        message = minimal_message_without_mac()
        frame = self.codec.encrypt_frame("downstream", self.header, message)
        document = self.codec.decode("downstream", frame)
        document["processing_code"] = {"type": "string", "value": "990000"}
        document["message_authentication_code"] = {
            "type": "blob",
            "value_base64": base64.b64encode(b"FAKEMAC!").decode(),
        }

        with self.assertRaisesRegex(ValueError, "MAC"):
            self.codec.encode("downstream", document)

    def test_encode_rejects_changes_to_a_secondary_bitmap_message_with_de128(self) -> None:
        message = encode_message(
            {
                "message_type": {"type": "string", "value": "1520"},
                "processing_code": {"type": "string", "value": "000000"},
                "message_authentication_code_extended": {
                    "type": "blob",
                    "value_base64": base64.b64encode(b"MAC_EXT!").decode(),
                },
            }
        )
        frame = self.codec.encrypt_frame("upstream", self.header, message)
        document = self.codec.decode("upstream", frame)
        document["processing_code"] = {"type": "string", "value": "990000"}

        with self.assertRaisesRegex(ValueError, "MAC"):
            self.codec.encode("upstream", document)

    def test_display_redacts_pan_and_extended_mac(self) -> None:
        message = encode_message(
            {
                "message_type": {"type": "string", "value": "1520"},
                "primary_account_number": {
                    "type": "string",
                    "value": "999999999",
                },
                "message_authentication_code_extended": {
                    "type": "blob",
                    "value_base64": base64.b64encode(b"MAC_EXT!").decode(),
                },
            }
        )
        frame = self.codec.encrypt_frame("upstream", self.header, message)
        document = self.codec.decode("upstream", frame)

        rendered = self.codec.display("upstream", document)

        self.assertNotIn("999999999", rendered)
        self.assertNotIn("MAC_EXT!", rendered)

    def test_display_decodes_de48_f0_pos_data_and_extended_transaction_type(self) -> None:
        de48 = (
            b"\xf0\x00\x19\x90\x00"
            b"TERM0001"
            b"000123"
            b"54321"
            b"9001"
        )
        message = encode_message(
            {
                "message_type": {"type": "string", "value": "1200"},
                "processing_code": {"type": "string", "value": "000000"},
                "additional_private": {
                    "type": "blob",
                    "value_base64": base64.b64encode(de48).decode(),
                },
            }
        )
        frame = self.codec.encrypt_frame("upstream", self.header, message)
        document = self.codec.decode("upstream", frame)

        rendered = self.codec.display("upstream", document)

        self.assertIn("F0.1 POS data", rendered)
        self.assertIn("terminal ID=TERM0001", rendered)
        self.assertIn("transaction number=000123", rendered)
        self.assertIn("F0.4 extended transaction type=9001", rendered)
        self.assertIn("operator ID=[redacted: 5 chars]", rendered)
        self.assertNotIn("54321", rendered)

    def test_display_falls_back_to_redaction_for_unknown_de48_subfields(self) -> None:
        de48 = b"\xf0\x00\x03\x20\x00\x00"
        message = encode_message(
            {
                "message_type": {"type": "string", "value": "1200"},
                "processing_code": {"type": "string", "value": "000000"},
                "additional_private": {
                    "type": "blob",
                    "value_base64": base64.b64encode(de48).decode(),
                },
            }
        )
        frame = self.codec.encrypt_frame("upstream", self.header, message)
        document = self.codec.decode("upstream", frame)

        rendered = self.codec.display("upstream", document)

        self.assertIn("[redacted blob: 6 bytes]", rendered)

    def test_display_falls_back_to_redaction_for_malformed_de48_length(self) -> None:
        de48 = b"\xf0\x00\x18\x90\x00" + b"TERM0001000123543219001"
        message = encode_message(
            {
                "message_type": {"type": "string", "value": "1200"},
                "processing_code": {"type": "string", "value": "000000"},
                "additional_private": {
                    "type": "blob",
                    "value_base64": base64.b64encode(de48).decode(),
                },
            }
        )
        frame = self.codec.encrypt_frame("upstream", self.header, message)
        document = self.codec.decode("upstream", frame)

        rendered = self.codec.display("upstream", document)

        self.assertIn("[redacted blob: 28 bytes]", rendered)

    def test_de48_display_parsing_does_not_change_wire_bytes(self) -> None:
        de48 = (
            b"\xf0\x00\x19\x90\x00"
            b"TERM0001"
            b"000123"
            b"54321"
            b"9001"
        )
        message = encode_message(
            {
                "message_type": {"type": "string", "value": "1200"},
                "processing_code": {"type": "string", "value": "000000"},
                "additional_private": {
                    "type": "blob",
                    "value_base64": base64.b64encode(de48).decode(),
                },
            }
        )
        frame = self.codec.encrypt_frame("upstream", self.header, message)
        document = self.codec.decode("upstream", frame)

        self.codec.display("upstream", document)

        self.assertEqual(self.codec.encode("upstream", document), frame)

    def test_encode_rejects_a_tampered_encoding_context(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)
        document = self.codec.decode("upstream", frame)
        document["encoding_context"] = {
            "type": "blob",
            "value_base64": base64.b64encode(b"tampered").decode(),
        }

        with self.assertRaisesRegex(ValueError, "context"):
            self.codec.encode("upstream", document)

    def test_encoding_context_does_not_expose_the_clear_frame(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)
        document = self.codec.decode("upstream", frame)

        context = base64.b64decode(document["encoding_context"]["value_base64"])

        self.assertNotIn(self.header, context)
        self.assertNotIn(self.message, context)

    def test_encoding_context_is_bound_to_direction(self) -> None:
        frame = self.codec.encrypt_frame("upstream", self.header, self.message)
        document = self.codec.decode("upstream", frame)

        with self.assertRaisesRegex(ValueError, "context"):
            self.codec.encode("downstream", document)


if __name__ == "__main__":
    unittest.main()
