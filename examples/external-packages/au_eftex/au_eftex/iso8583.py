"""Bounded AU EFTEX ISO 8583:1993 parser and encoder.

Only the fields declared in :data:`FIELD_SCHEMA` are understood. Binary
payment fields remain opaque blobs; this module does not interpret PIN, MAC,
or private field contents.
"""

from __future__ import annotations

import base64
import binascii
from dataclasses import dataclass
from typing import Literal, TypedDict, Union


class StringValue(TypedDict):
    type: Literal["string"]
    value: str


class IntValue(TypedDict):
    type: Literal["int"]
    value: str


class BlobValue(TypedDict):
    type: Literal["blob"]
    value_base64: str


DocumentValue = Union[StringValue, IntValue, BlobValue]
Document = dict[str, DocumentValue]

FieldKind = Literal[
    "fixed_ascii",
    "fixed_amount",
    "fixed_blob",
    "ll_ascii",
    "ll_blob",
    "lll_ascii",
    "lll_blob",
    "llll_ascii",
    "llll_blob",
]
DocumentType = Literal["string", "int", "blob"]


@dataclass(frozen=True, slots=True)
class FieldSpec:
    number: int
    name: str
    kind: FieldKind
    length: int
    document_type: DocumentType


FIELD_SCHEMA: tuple[FieldSpec, ...] = (
    FieldSpec(0, "message_type", "fixed_ascii", 4, "string"),
    FieldSpec(2, "primary_account_number", "ll_ascii", 19, "string"),
    FieldSpec(3, "processing_code", "fixed_ascii", 6, "string"),
    FieldSpec(4, "amount", "fixed_amount", 12, "int"),
    FieldSpec(7, "transmission_time", "fixed_ascii", 10, "string"),
    FieldSpec(11, "stan", "fixed_ascii", 6, "string"),
    FieldSpec(12, "local_transaction_time", "fixed_ascii", 12, "string"),
    FieldSpec(14, "expiration_date", "fixed_ascii", 4, "string"),
    FieldSpec(22, "pos_data_code", "fixed_ascii", 15, "string"),
    FieldSpec(23, "card_sequence_number", "fixed_ascii", 3, "string"),
    FieldSpec(24, "function_code", "fixed_ascii", 3, "string"),
    FieldSpec(25, "message_reason_code", "fixed_ascii", 4, "string"),
    FieldSpec(28, "reconciliation_date", "fixed_ascii", 6, "string"),
    FieldSpec(29, "reconciliation_indicator", "fixed_ascii", 3, "string"),
    FieldSpec(35, "track_2_data", "ll_ascii", 37, "string"),
    FieldSpec(37, "retrieval_reference_number", "fixed_ascii", 12, "string"),
    FieldSpec(38, "approval_code", "fixed_ascii", 6, "string"),
    FieldSpec(39, "action_code", "fixed_ascii", 3, "string"),
    FieldSpec(40, "service_code", "fixed_ascii", 3, "string"),
    FieldSpec(41, "terminal_id", "fixed_ascii", 8, "string"),
    FieldSpec(42, "card_acceptor_id", "fixed_ascii", 15, "string"),
    FieldSpec(46, "amounts_fees", "lll_ascii", 204, "string"),
    FieldSpec(48, "additional_private", "llll_blob", 9_999, "blob"),
    FieldSpec(49, "currency", "fixed_ascii", 3, "string"),
    FieldSpec(50, "reconciliation_currency", "fixed_ascii", 3, "string"),
    FieldSpec(52, "pin_data", "fixed_blob", 8, "blob"),
    FieldSpec(53, "security_control_information", "ll_ascii", 48, "string"),
    FieldSpec(54, "additional_amounts", "lll_ascii", 120, "string"),
    FieldSpec(55, "icc_data", "lll_blob", 512, "blob"),
    FieldSpec(56, "original_data_elements", "ll_ascii", 31, "string"),
    FieldSpec(63, "reserved_private", "lll_ascii", 800, "string"),
    FieldSpec(64, "message_authentication_code", "fixed_blob", 8, "blob"),
    FieldSpec(74, "credits_number", "fixed_ascii", 10, "string"),
    FieldSpec(75, "credits_reversal_number", "fixed_ascii", 10, "string"),
    FieldSpec(76, "debits_number", "fixed_ascii", 10, "string"),
    FieldSpec(77, "debits_reversal_number", "fixed_ascii", 10, "string"),
    FieldSpec(81, "authorisations_number", "fixed_ascii", 10, "string"),
    FieldSpec(86, "credits_amount", "fixed_ascii", 16, "string"),
    FieldSpec(87, "credits_reversal_amount", "fixed_ascii", 16, "string"),
    FieldSpec(88, "debits_amount", "fixed_ascii", 16, "string"),
    FieldSpec(89, "debits_reversal_amount", "fixed_ascii", 16, "string"),
    FieldSpec(90, "authorisations_reversal_number", "fixed_ascii", 10, "string"),
    FieldSpec(97, "net_reconciliation_amount", "fixed_ascii", 17, "string"),
    FieldSpec(109, "credits_fee_amounts", "ll_ascii", 84, "string"),
    FieldSpec(110, "debits_fee_amounts", "ll_ascii", 84, "string"),
    FieldSpec(123, "receipt_data", "lll_ascii", 999, "string"),
    FieldSpec(124, "display_data", "lll_ascii", 999, "string"),
    FieldSpec(128, "message_authentication_code_extended", "fixed_blob", 8, "blob"),
)

_MESSAGE_TYPE = FIELD_SCHEMA[0]
_DATA_FIELDS = FIELD_SCHEMA[1:]
_FIELDS_BY_NUMBER = {field.number: field for field in _DATA_FIELDS}
_FIELDS_BY_NAME = {field.name: field for field in FIELD_SCHEMA}


def message_length(message: bytes) -> int | None:
    """Return one complete message length, or ``None`` when more bytes are needed."""

    _require_bytes(message)
    if len(message) < _MESSAGE_TYPE.length:
        return None
    _read_mti(message[:4])
    if len(message) < 12:
        return None

    primary_bitmap = message[4:12]
    bitmap_bytes = 16 if _bitmap_has(primary_bitmap, 1) else 8
    if len(message) < 4 + bitmap_bytes:
        return None
    bitmap = message[4 : 4 + bitmap_bytes]
    _validate_bitmap(bitmap)
    offset = 4 + bitmap_bytes
    maximum = 128 if bitmap_bytes == 16 else 64
    for number in range(2, maximum + 1):
        if number == 65:
            continue
        if not _bitmap_has(bitmap, number):
            continue
        field = _FIELDS_BY_NUMBER[number]
        payload_length, prefix_length = _field_length(message, offset, field)
        if payload_length is None:
            return None
        end = offset + prefix_length + payload_length
        if len(message) < end:
            return None
        if field.document_type != "blob":
            value = _decode_ascii(
                message[offset + prefix_length : end], field.name
            )
            if field.kind == "fixed_amount":
                _require_digits(value, field.name)
        offset = end
    return offset


def decode_message(message: bytes) -> Document:
    """Decode one complete profile message into tagged document values."""

    expected_length = message_length(message)
    if expected_length is None:
        raise ValueError("ISO 8583 message is incomplete")
    if expected_length != len(message):
        raise ValueError("ISO 8583 message has trailing bytes not described by bitmap")

    document: Document = {
        "message_type": {"type": "string", "value": _read_mti(message[:4])}
    }
    primary_bitmap = message[4:12]
    bitmap_bytes = 16 if _bitmap_has(primary_bitmap, 1) else 8
    bitmap = message[4 : 4 + bitmap_bytes]
    offset = 4 + bitmap_bytes
    maximum = 128 if bitmap_bytes == 16 else 64
    for number in range(2, maximum + 1):
        if number == 65:
            continue
        if not _bitmap_has(bitmap, number):
            continue
        field = _FIELDS_BY_NUMBER[number]
        payload_length, prefix_length = _field_length(message, offset, field)
        assert payload_length is not None
        start = offset + prefix_length
        end = start + payload_length
        payload = message[start:end]
        if field.document_type == "blob":
            document[field.name] = {
                "type": "blob",
                "value_base64": base64.b64encode(payload).decode("ascii"),
            }
        else:
            value = _decode_ascii(payload, field.name)
            document[field.name] = (
                {"type": "int", "value": _canonical_integer(value, field.name)}
                if field.document_type == "int"
                else {"type": "string", "value": value}
            )
        offset = end
    return document


def encode_message(document: Document) -> bytes:
    """Encode a tagged document as one profile message without a wire header."""

    if not isinstance(document, dict):
        raise TypeError("document must be a dict")
    unknown = sorted(set(document) - set(_FIELDS_BY_NAME))
    if unknown:
        raise ValueError(f"unknown document field: {unknown[0]}")
    if "message_type" not in document:
        raise ValueError("message_type is required")

    mti = _require_string_value(document["message_type"], "message_type")
    if len(mti) != 4:
        raise ValueError("message_type must contain exactly 4 ASCII digits")
    _require_digits(mti, "message_type")
    mti_bytes = _encode_ascii(mti, "message_type")

    present = [field for field in _DATA_FIELDS if field.name in document]
    has_secondary = any(field.number > 64 for field in present)
    bitmap = bytearray(16 if has_secondary else 8)
    if has_secondary:
        _bitmap_set(bitmap, 1)
    encoded_fields: list[bytes] = []
    for field in present:
        _bitmap_set(bitmap, field.number)
        encoded_fields.append(_encode_field(document[field.name], field))
    return b"".join([mti_bytes, bytes(bitmap), *encoded_fields])


def _validate_bitmap(bitmap: bytes) -> None:
    if len(bitmap) not in {8, 16}:
        raise ValueError("ISO 8583 bitmap must contain 8 or 16 bytes")
    if len(bitmap) == 16 and _bitmap_has(bitmap, 65):
        raise ValueError("tertiary bitmap is not supported by the AU EFTEX profile")
    maximum = 128 if len(bitmap) == 16 else 64
    for number in range(2, maximum + 1):
        if number == 65:
            continue
        if _bitmap_has(bitmap, number) and number not in _FIELDS_BY_NUMBER:
            raise ValueError(f"DE{number} is not supported by the AU EFTEX profile")


def _field_length(
    message: bytes, offset: int, field: FieldSpec
) -> tuple[int | None, int]:
    prefix_length = {
        "ll_ascii": 2,
        "ll_blob": 2,
        "lll_ascii": 3,
        "lll_blob": 3,
        "llll_ascii": 4,
        "llll_blob": 4,
    }.get(field.kind, 0)
    if prefix_length == 0:
        return field.length, 0
    if len(message) < offset + prefix_length:
        return None, prefix_length
    prefix = _decode_ascii(
        message[offset : offset + prefix_length], f"{field.name} length"
    )
    _require_digits(prefix, f"{field.name} length")
    length = int(prefix)
    if length > field.length:
        raise ValueError(f"{field.name} length exceeds profile maximum")
    return length, prefix_length


def _encode_field(value: DocumentValue, field: FieldSpec) -> bytes:
    if field.document_type == "blob":
        payload = _require_blob_value(value, field.name)
    elif field.document_type == "int":
        integer = _require_int_value(value, field.name)
        if len(integer) > field.length:
            raise ValueError(f"{field.name} exceeds {field.length} digits")
        payload = integer.zfill(field.length).encode("ascii")
    else:
        payload = _encode_ascii(_require_string_value(value, field.name), field.name)

    prefix_length = {
        "ll_ascii": 2,
        "ll_blob": 2,
        "lll_ascii": 3,
        "lll_blob": 3,
        "llll_ascii": 4,
        "llll_blob": 4,
    }.get(field.kind, 0)
    if prefix_length:
        if len(payload) > field.length:
            raise ValueError(f"{field.name} length exceeds profile maximum")
        return f"{len(payload):0{prefix_length}d}".encode("ascii") + payload
    if len(payload) != field.length:
        raise ValueError(f"{field.name} must contain exactly {field.length} bytes")
    return payload


def _require_string_value(value: object, name: str) -> str:
    if (
        not isinstance(value, dict)
        or set(value) != {"type", "value"}
        or value.get("type") != "string"
        or not isinstance(value.get("value"), str)
    ):
        raise ValueError(f"{name} must be a closed string tagged value")
    return value["value"]


def _require_int_value(value: object, name: str) -> str:
    if (
        not isinstance(value, dict)
        or set(value) != {"type", "value"}
        or value.get("type") != "int"
        or not isinstance(value.get("value"), str)
    ):
        raise ValueError(f"{name} must be a closed int tagged value")
    integer = value["value"]
    if not integer.isascii() or not integer.isdecimal():
        raise ValueError(f"{name} must contain ASCII digits")
    if len(integer) > 1 and integer.startswith("0"):
        raise ValueError(f"{name} must use a canonical integer string")
    return integer


def _require_blob_value(value: object, name: str) -> bytes:
    if (
        not isinstance(value, dict)
        or set(value) != {"type", "value_base64"}
        or value.get("type") != "blob"
        or not isinstance(value.get("value_base64"), str)
    ):
        raise ValueError(f"{name} must be a closed blob tagged value")
    try:
        return base64.b64decode(value["value_base64"], validate=True)
    except (binascii.Error, ValueError) as error:
        raise ValueError(f"{name} must contain valid base64") from error


def _read_mti(payload: bytes) -> str:
    mti = _decode_ascii(payload, "MTI")
    _require_digits(mti, "MTI")
    return mti


def _canonical_integer(value: str, name: str) -> str:
    _require_digits(value, name)
    return value.lstrip("0") or "0"


def _require_digits(value: str, name: str) -> None:
    if not value.isascii() or not value.isdecimal():
        raise ValueError(f"{name} must contain ASCII digits")


def _decode_ascii(payload: bytes, name: str) -> str:
    try:
        return payload.decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError(f"{name} must contain ASCII bytes") from error


def _encode_ascii(value: str, name: str) -> bytes:
    try:
        return value.encode("ascii")
    except UnicodeEncodeError as error:
        raise ValueError(f"{name} must contain ASCII characters") from error


def _bitmap_has(bitmap: bytes, number: int) -> bool:
    index = number - 1
    return bool(bitmap[index // 8] & (1 << (7 - index % 8)))


def _bitmap_set(bitmap: bytearray, number: int) -> None:
    index = number - 1
    bitmap[index // 8] |= 1 << (7 - index % 8)


def _require_bytes(message: object) -> None:
    if not isinstance(message, bytes):
        raise TypeError("message must be bytes")
