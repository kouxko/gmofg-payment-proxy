from __future__ import annotations

import base64
import unittest

from au_eftex.iso8583 import (
    FIELD_SCHEMA,
    decode_message,
    encode_message,
    message_length,
)


BITMAP = bytes.fromhex("323005182bc19c01")
PRIVATE_DATA = b"\x00\xffopaque"
PIN_DATA = b"PINTEST!"
MAC = b"MAC_TEST"
EXTENDED_MAC = b"MAC_EXT!"


def synthetic_message() -> bytes:
    return b"".join(
        [
            b"1200",
            BITMAP,
            b"990000",
            b"000000000042",
            b"0102030405",
            b"000123",
            b"010203040506",
            b"ABCDEFGHIJKLMNO",
            b"200",
            b"123456",
            b"001",
            b"12SYNTHETIC=01",
            b"REF000000001",
            b"000",
            b"XYZ",
            b"TERM0001",
            b"SYNTHETIC000001",
            b"0008" + PRIVATE_DATA,
            b"036",
            PIN_DATA,
            b"10SECURITY01",
            b"009SYNTHETIC",
            MAC,
        ]
    )


def string_value(value: str) -> dict[str, str]:
    return {"type": "string", "value": value}


def blob_value(value: bytes) -> dict[str, str]:
    return {
        "type": "blob",
        "value_base64": base64.b64encode(value).decode("ascii"),
    }


def synthetic_document() -> dict[str, dict[str, str]]:
    return {
        "message_type": string_value("1200"),
        "processing_code": string_value("990000"),
        "amount": {"type": "int", "value": "42"},
        "transmission_time": string_value("0102030405"),
        "stan": string_value("000123"),
        "local_transaction_time": string_value("010203040506"),
        "pos_data_code": string_value("ABCDEFGHIJKLMNO"),
        "function_code": string_value("200"),
        "reconciliation_date": string_value("123456"),
        "reconciliation_indicator": string_value("001"),
        "track_2_data": string_value("SYNTHETIC=01"),
        "retrieval_reference_number": string_value("REF000000001"),
        "action_code": string_value("000"),
        "service_code": string_value("XYZ"),
        "terminal_id": string_value("TERM0001"),
        "card_acceptor_id": string_value("SYNTHETIC000001"),
        "additional_private": blob_value(PRIVATE_DATA),
        "currency": string_value("036"),
        "pin_data": blob_value(PIN_DATA),
        "security_control_information": string_value("SECURITY01"),
        "additional_amounts": string_value("SYNTHETIC"),
        "message_authentication_code": blob_value(MAC),
    }


class FieldSchemaTests(unittest.TestCase):
    def test_schema_uses_the_confirmed_mti_and_data_element_order(self) -> None:
        self.assertEqual(
            [(field.number, field.name) for field in FIELD_SCHEMA],
            [
                (0, "message_type"),
                (2, "primary_account_number"),
                (3, "processing_code"),
                (4, "amount"),
                (7, "transmission_time"),
                (11, "stan"),
                (12, "local_transaction_time"),
                (14, "expiration_date"),
                (22, "pos_data_code"),
                (23, "card_sequence_number"),
                (24, "function_code"),
                (25, "message_reason_code"),
                (28, "reconciliation_date"),
                (29, "reconciliation_indicator"),
                (35, "track_2_data"),
                (37, "retrieval_reference_number"),
                (38, "approval_code"),
                (39, "action_code"),
                (40, "service_code"),
                (41, "terminal_id"),
                (42, "card_acceptor_id"),
                (46, "amounts_fees"),
                (48, "additional_private"),
                (49, "currency"),
                (50, "reconciliation_currency"),
                (52, "pin_data"),
                (53, "security_control_information"),
                (54, "additional_amounts"),
                (55, "icc_data"),
                (56, "original_data_elements"),
                (63, "reserved_private"),
                (64, "message_authentication_code"),
                (74, "credits_number"),
                (75, "credits_reversal_number"),
                (76, "debits_number"),
                (77, "debits_reversal_number"),
                (81, "authorisations_number"),
                (86, "credits_amount"),
                (87, "credits_reversal_amount"),
                (88, "debits_amount"),
                (89, "debits_reversal_amount"),
                (90, "authorisations_reversal_number"),
                (97, "net_reconciliation_amount"),
                (109, "credits_fee_amounts"),
                (110, "debits_fee_amounts"),
                (123, "receipt_data"),
                (124, "display_data"),
                (128, "message_authentication_code_extended"),
            ],
        )


class Iso8583RoundTripTests(unittest.TestCase):
    def test_decode_and_encode_preserve_the_exact_message(self) -> None:
        message = synthetic_message()

        document = decode_message(message)

        self.assertEqual(document, synthetic_document())
        self.assertEqual(encode_message(document), message)

    def test_amount_decodes_canonically_and_encodes_as_twelve_digits(self) -> None:
        document = synthetic_document()

        encoded = encode_message(document)

        self.assertIn(b"000000000042", encoded)
        self.assertEqual(
            decode_message(encoded)["amount"], {"type": "int", "value": "42"}
        )

    def test_amount_rejects_a_noncanonical_integer_document_value(self) -> None:
        document = synthetic_document()
        document["amount"] = {"type": "int", "value": "00042"}

        with self.assertRaisesRegex(ValueError, "canonical integer string"):
            encode_message(document)

    def test_message_length_supports_partial_and_sticky_buffers(self) -> None:
        message = synthetic_message()

        for end in (0, 3, 11, 12, len(message) - 1):
            self.assertIsNone(message_length(message[:end]))
        self.assertEqual(message_length(message), len(message))
        self.assertEqual(message_length(message + b"next-message"), len(message))

    def test_secondary_bitmap_binary_field_and_extended_mac_round_trip(self) -> None:
        document = {
            "message_type": string_value("1520"),
            "primary_account_number": string_value("999999999"),
            "icc_data": blob_value(b"\x00\xff"),
            "credits_number": string_value("0000000001"),
            "message_authentication_code_extended": blob_value(EXTENDED_MAC),
        }
        expected = b"".join(
            [
                b"1520",
                bytes.fromhex("c0000000000002000040000000000001"),
                b"09" + b"999999999",
                b"002" + b"\x00\xff",
                b"0000000001",
                EXTENDED_MAC,
            ]
        )

        self.assertEqual(encode_message(document), expected)
        self.assertEqual(decode_message(expected), document)
        self.assertIsNone(message_length(expected[:19]))
        self.assertEqual(message_length(expected + b"sticky"), len(expected))


class Iso8583ValidationTests(unittest.TestCase):
    def test_encode_rejects_missing_and_unknown_document_fields(self) -> None:
        with self.assertRaisesRegex(ValueError, "message_type is required"):
            encode_message({})

        document = synthetic_document()
        document["unexpected"] = {"type": "string", "value": "x"}
        with self.assertRaisesRegex(ValueError, "unknown document field"):
            encode_message(document)

    def test_encode_rejects_values_outside_the_closed_tagged_union(self) -> None:
        document = synthetic_document()
        document["processing_code"] = {
            "type": "string",
            "value": "990000",
            "extra": "not allowed",
        }

        with self.assertRaisesRegex(ValueError, "processing_code.*tagged value"):
            encode_message(document)

    def test_bitmap_rejects_tertiary_and_unknown_fields(self) -> None:
        with self.assertRaisesRegex(ValueError, "tertiary bitmap"):
            message_length(
                b"1200" + bytes.fromhex("80000000000000008000000000000000")
            )

        with self.assertRaisesRegex(ValueError, "DE5.*not supported"):
            message_length(b"1200" + bytes.fromhex("0800000000000000"))

        with self.assertRaisesRegex(ValueError, "DE66.*not supported"):
            message_length(
                b"1200" + bytes.fromhex("80000000000000004000000000000000")
            )

    def test_xml_variable_length_maxima_are_enforced(self) -> None:
        document = {
            "message_type": string_value("1200"),
            "track_2_data": string_value("X" * 38),
        }
        with self.assertRaisesRegex(ValueError, "track_2_data length"):
            encode_message(document)

    def test_invalid_length_prefix_and_trailing_bytes_are_rejected(self) -> None:
        track_2_only = b"1200" + bytes.fromhex("0000000020000000")
        with self.assertRaisesRegex(ValueError, "track_2_data length.*ASCII digits"):
            message_length(track_2_only + b"X1")

        with self.assertRaisesRegex(ValueError, "trailing bytes"):
            decode_message(synthetic_message() + b"x")

    def test_truncated_message_is_not_decodable(self) -> None:
        with self.assertRaisesRegex(ValueError, "incomplete"):
            decode_message(synthetic_message()[:-1])

    def test_non_ascii_mti_and_field_data_are_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "MTI.*ASCII"):
            message_length(b"12\xff0" + bytes(8))

        processing_code_only = b"1200" + bytes.fromhex("2000000000000000")
        with self.assertRaisesRegex(ValueError, "processing_code.*ASCII"):
            decode_message(processing_code_only + b"12345\xff")

        document = synthetic_document()
        document["terminal_id"] = {"type": "string", "value": "终端000001"}
        with self.assertRaisesRegex(ValueError, "terminal_id.*ASCII"):
            encode_message(document)


if __name__ == "__main__":
    unittest.main()
