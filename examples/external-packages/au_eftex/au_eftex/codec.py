from __future__ import annotations

import base64
import binascii
import html
import secrets
from dataclasses import dataclass
from typing import Literal

from Crypto.Cipher import AES

from .crypto import (
    derive_data_request_key,
    derive_data_response_key,
    derive_ipek,
    derive_transaction_key,
    tdes_ofb_decrypt,
    tdes_ofb_encrypt,
)
from .iso8583 import FIELD_SCHEMA, decode_message, encode_message, message_length


Direction = Literal["upstream", "downstream"]
LengthPrefixMode = Literal["none", "u16_be_body", "u16_be_total"]
HEADER_BYTES = 39
MAX_FRAME_BYTES = 65_535
ENCODING_CONTEXT_FIELD = "encoding_context"
_CONTEXT_MAGIC = b"AUE2"
_CONTEXT_NONCE_BYTES = 12
_CONTEXT_TAG_BYTES = 16
_DATA_IV_MASK = bytes.fromhex("0123456789ABCDEF")
_PREFIX_MODE_BYTES: dict[LengthPrefixMode, bytes] = {
    "none": b"\x00",
    "u16_be_body": b"\x01",
    "u16_be_total": b"\x02",
}
_PREFIX_BYTES_MODES: dict[bytes, LengthPrefixMode] = {
    value: key for key, value in _PREFIX_MODE_BYTES.items()
}
_SENSITIVE_DISPLAY_FIELDS = {
    "primary_account_number",
    "track_2_data",
    "pin_data",
    "security_control_information",
    "message_authentication_code",
    "message_authentication_code_extended",
    "additional_private",
    "reserved_private",
    "receipt_data",
    "display_data",
}
_MAC_FIELDS = {
    "message_authentication_code",
    "message_authentication_code_extended",
}


@dataclass(frozen=True)
class DecryptedFrame:
    header: bytes
    message: bytes
    length_prefix_mode: LengthPrefixMode = "none"


def pad_message(message: bytes) -> bytes:
    total_padding = 8 - (len(message) % 8)
    fill_count = total_padding - 1
    return message + (b"\xff" * fill_count) + bytes([fill_count])


def unpad_message(padded: bytes) -> bytes:
    if not padded or len(padded) % 8:
        raise ValueError("invalid EFTEX padding length")
    fill_count = padded[-1]
    if fill_count > 7 or len(padded) < fill_count + 1:
        raise ValueError("invalid EFTEX padding count")
    if padded[-fill_count - 1 : -1] != b"\xff" * fill_count:
        raise ValueError("invalid EFTEX padding fill bytes")
    return padded[: -fill_count - 1]


class EftexCodec:
    def __init__(self, *, bdk: bytes, context_key: bytes | None = None) -> None:
        if len(bdk) != 16:
            raise ValueError("AU EFTEX BDK must be 16 bytes")
        self._bdk = bytes(bdk)
        self._context_key = bytes(context_key) if context_key is not None else secrets.token_bytes(32)
        if len(self._context_key) != 32:
            raise ValueError("encoding context key must be 32 bytes")

    def frame(self, direction: str, buffer: bytes) -> dict[str, object]:
        return self.frame_boundary(direction, buffer)

    def frame_boundary(self, direction: str, buffer: bytes) -> dict[str, object]:
        normalized = _direction(direction)
        if len(buffer) < HEADER_BYTES:
            return {"status": "need_more"}
        header_offset = _header_offset(buffer)
        if header_offset is None:
            return {"status": "need_more"}
        header = buffer[header_offset : header_offset + HEADER_BYTES]
        ksn = _parse_header(header)
        if header_offset:
            _, frame_bytes = _prefixed_frame_size(buffer[:2])
            if len(buffer) < frame_bytes:
                return {"status": "need_more"}
            return {"status": "complete", "consumed_bytes": frame_bytes}
        encrypted = buffer[header_offset + HEADER_BYTES :]
        if len(encrypted) < 12:
            return {"status": "need_more"}
        iv = _data_iv(header)
        clear_prefix = tdes_ofb_decrypt(self._data_key(normalized, ksn), iv, encrypted)
        try:
            clear_bytes = message_length(clear_prefix)
        except ValueError as error:
            if _plausible_mti(clear_prefix):
                raise
            self._raise_invalid_mti_diagnostic(normalized, ksn, iv, encrypted, error)
        if clear_bytes is None:
            return {"status": "need_more"}
        encrypted_bytes = clear_bytes + (8 - clear_bytes % 8)
        body_bytes = HEADER_BYTES + encrypted_bytes
        frame_bytes = header_offset + body_bytes
        if frame_bytes > MAX_FRAME_BYTES:
            raise ValueError("AU EFTEX frame exceeds 65,535 bytes")
        if header_offset:
            _length_prefix_mode(buffer[:2], body_bytes, frame_bytes)
        if len(buffer) < frame_bytes:
            return {"status": "need_more"}
        return {"status": "complete", "consumed_bytes": frame_bytes}

    def decrypt_frame(self, direction: str, frame: bytes) -> DecryptedFrame:
        normalized = _direction(direction)
        if len(frame) < HEADER_BYTES + 8 or len(frame) > MAX_FRAME_BYTES:
            raise ValueError("invalid AU EFTEX frame length")
        header_offset = _header_offset(frame)
        if header_offset is None:
            raise ValueError("invalid AU EFTEX frame length")
        header = frame[header_offset : header_offset + HEADER_BYTES]
        ksn = _parse_header(header)
        encrypted = frame[header_offset + HEADER_BYTES :]
        if len(encrypted) % 8:
            raise ValueError("AU EFTEX encrypted message must be block aligned")
        length_prefix_mode: LengthPrefixMode = "none"
        if header_offset:
            length_prefix_mode = _length_prefix_mode(
                frame[:2],
                len(frame) - header_offset,
                len(frame),
            )
        iv = _data_iv(header)
        padded = tdes_ofb_decrypt(self._data_key(normalized, ksn), iv, encrypted)
        if not _plausible_mti(padded):
            self._raise_invalid_mti_diagnostic(
                normalized,
                ksn,
                iv,
                encrypted,
                ValueError("AU EFTEX decrypted MTI is invalid"),
            )
        message = unpad_message(padded)
        parsed_bytes = message_length(message)
        if parsed_bytes is None or parsed_bytes != len(message):
            raise ValueError("AU EFTEX message length does not match its bitmap fields")
        return DecryptedFrame(
            header=header,
            message=message,
            length_prefix_mode=length_prefix_mode,
        )

    def encrypt_frame(
        self,
        direction: str,
        header: bytes,
        message: bytes,
        *,
        length_prefix_mode: LengthPrefixMode = "none",
    ) -> bytes:
        normalized = _direction(direction)
        ksn = _parse_header(header)
        parsed_bytes = message_length(message)
        if parsed_bytes is None or parsed_bytes != len(message):
            raise ValueError("AU EFTEX message length does not match its bitmap fields")
        encrypted = tdes_ofb_encrypt(
            self._data_key(normalized, ksn),
            _data_iv(header),
            pad_message(message),
        )
        body = header + encrypted
        frame = _apply_length_prefix(body, length_prefix_mode)
        if len(frame) > MAX_FRAME_BYTES:
            raise ValueError("AU EFTEX frame exceeds 65,535 bytes")
        return frame

    def decode(self, direction: str, frame: bytes) -> dict[str, object]:
        normalized = _direction(direction)
        clear = self.decrypt_frame(normalized, frame)
        document: dict[str, object] = dict(decode_message(clear.message))
        document[ENCODING_CONTEXT_FIELD] = self._encode_context(
            normalized,
            clear.length_prefix_mode,
            clear.header + clear.message,
        )
        return document

    def encode(self, direction: str, document: dict[str, object]) -> bytes:
        normalized = _direction(direction)
        length_prefix_mode, header, original_message = self._decode_context(normalized, document)
        fields = {name: value for name, value in document.items() if name != ENCODING_CONTEXT_FIELD}
        message = encode_message(fields)  # type: ignore[arg-type]
        if message != original_message:
            original = decode_message(original_message)
            if any(name in original or name in fields for name in _MAC_FIELDS):
                raise ValueError(
                    "ISO 8583 fields changed but replacement MAC validation is unavailable"
                )
        return self.encrypt_frame(
            normalized,
            header,
            message,
            length_prefix_mode=length_prefix_mode,
        )

    def display(self, direction: str, document: dict[str, object]) -> str:
        normalized = _direction(direction)
        self._decode_context(normalized, document)
        fields = {name: value for name, value in document.items() if name != ENCODING_CONTEXT_FIELD}
        encode_message(fields)  # type: ignore[arg-type]
        label = "Upstream" if normalized == "upstream" else "Downstream"
        rows = [f"<tr><th>Direction</th><td>{html.escape(label)}</td></tr>"]
        for field in FIELD_SCHEMA:
            value = fields.get(field.name)
            if value is None:
                continue
            rows.append(
                f"<tr><th>DE{field.number if field.number else 'MTI'} {html.escape(field.name)}</th>"
                f"<td>{html.escape(_display_value(field.name, value))}</td></tr>"
            )
        return (
            '<section class="protocol-document"><h3>AU EFTEX ISO 8583</h3><table><tbody>'
            + "".join(rows)
            + "</tbody></table></section>"
        )

    def _encode_context(
        self,
        direction: Direction,
        length_prefix_mode: LengthPrefixMode,
        clear_frame: bytes,
    ) -> dict[str, str]:
        nonce = secrets.token_bytes(_CONTEXT_NONCE_BYTES)
        cipher = AES.new(self._context_key, AES.MODE_GCM, nonce=nonce, mac_len=_CONTEXT_TAG_BYTES)
        cipher.update(_context_aad(direction))
        ciphertext, tag = cipher.encrypt_and_digest(
            _PREFIX_MODE_BYTES[length_prefix_mode] + clear_frame
        )
        payload = _CONTEXT_MAGIC + nonce + ciphertext + tag
        return {
            "type": "blob",
            "value_base64": base64.b64encode(payload).decode("ascii"),
        }

    def _decode_context(
        self,
        direction: Direction,
        document: dict[str, object],
    ) -> tuple[LengthPrefixMode, bytes, bytes]:
        value = document.get(ENCODING_CONTEXT_FIELD)
        if not isinstance(value, dict) or set(value) != {"type", "value_base64"}:
            raise ValueError("encoding context must be a canonical blob")
        encoded = value.get("value_base64")
        if value.get("type") != "blob" or not isinstance(encoded, str):
            raise ValueError("encoding context must be a canonical blob")
        try:
            context = base64.b64decode(encoded, validate=True)
        except (binascii.Error, ValueError) as error:
            raise ValueError("encoding context contains invalid Base64") from error
        if base64.b64encode(context).decode("ascii") != encoded:
            raise ValueError("encoding context Base64 must be canonical")
        minimum = (
            len(_CONTEXT_MAGIC)
            + _CONTEXT_NONCE_BYTES
            + 1
            + HEADER_BYTES
            + 12
            + _CONTEXT_TAG_BYTES
        )
        if len(context) < minimum or context[:4] != _CONTEXT_MAGIC:
            raise ValueError("encoding context is invalid")
        nonce_end = len(_CONTEXT_MAGIC) + _CONTEXT_NONCE_BYTES
        nonce = context[len(_CONTEXT_MAGIC) : nonce_end]
        ciphertext = context[nonce_end:-_CONTEXT_TAG_BYTES]
        tag = context[-_CONTEXT_TAG_BYTES:]
        cipher = AES.new(self._context_key, AES.MODE_GCM, nonce=nonce, mac_len=_CONTEXT_TAG_BYTES)
        cipher.update(_context_aad(direction))
        try:
            clear_frame = cipher.decrypt_and_verify(ciphertext, tag)
        except ValueError as error:
            raise ValueError("encoding context authentication failed")
        if len(clear_frame) < 1 + HEADER_BYTES + 12:
            raise ValueError("encoding context frame is incomplete")
        try:
            length_prefix_mode = _PREFIX_BYTES_MODES[clear_frame[:1]]
        except KeyError as error:
            raise ValueError("encoding context length prefix mode is invalid") from error
        header = clear_frame[1 : 1 + HEADER_BYTES]
        _parse_header(header)
        original_message = clear_frame[1 + HEADER_BYTES :]
        decode_message(original_message)
        return length_prefix_mode, header, original_message

    def _data_key(self, direction: Direction, ksn: bytes) -> bytes:
        transaction_key = derive_transaction_key(derive_ipek(self._bdk, ksn), ksn)
        return (
            derive_data_request_key(transaction_key)
            if direction == "upstream"
            else derive_data_response_key(transaction_key)
        )

    def _raise_invalid_mti_diagnostic(
        self,
        direction: Direction,
        ksn: bytes,
        iv: bytes,
        encrypted: bytes,
        cause: ValueError,
    ) -> None:
        opposite: Direction = "downstream" if direction == "upstream" else "upstream"
        opposite_clear = tdes_ofb_decrypt(
            self._data_key(opposite, ksn),
            iv,
            encrypted[:4],
        )
        if _plausible_mti(opposite_clear):
            raise ValueError(
                "AU EFTEX data key direction does not match the hook direction"
            ) from cause
        if _plausible_mti(encrypted):
            raise ValueError("AU EFTEX payload is unexpectedly not encrypted") from cause
        raise ValueError("AU EFTEX decrypted MTI is invalid") from cause


def _direction(value: str) -> Direction:
    if value not in {"upstream", "downstream"}:
        raise ValueError("direction must be upstream or downstream")
    return value  # type: ignore[return-value]


def _plausible_mti(value: bytes) -> bool:
    return len(value) >= 4 and all(0x30 <= byte <= 0x39 for byte in value[:4])


def _context_aad(direction: Direction) -> bytes:
    return _CONTEXT_MAGIC + b":" + direction.encode("ascii")


def _header_offset(buffer: bytes) -> int | None:
    if buffer[:1] == b"T":
        return 0
    if len(buffer) < 2 + HEADER_BYTES:
        return None
    _parse_header(buffer[2 : 2 + HEADER_BYTES])
    return 2


def _length_prefix_mode(
    prefix: bytes,
    body_bytes: int,
    total_bytes: int,
) -> LengthPrefixMode:
    if len(prefix) != 2:
        raise ValueError("AU EFTEX length prefix must be 2 bytes")
    declared = int.from_bytes(prefix, "big")
    if declared == body_bytes:
        return "u16_be_body"
    if declared == total_bytes:
        return "u16_be_total"
    raise ValueError("AU EFTEX length prefix does not match the complete frame")


def _prefixed_frame_size(prefix: bytes) -> tuple[LengthPrefixMode, int]:
    if len(prefix) != 2:
        raise ValueError("AU EFTEX length prefix must be 2 bytes")
    declared = int.from_bytes(prefix, "big")
    candidates: list[tuple[LengthPrefixMode, int]] = [
        ("u16_be_body", declared + 2),
        ("u16_be_total", declared),
    ]
    valid = [
        (mode, total_bytes)
        for mode, total_bytes in candidates
        if 2 + HEADER_BYTES + 16 <= total_bytes <= MAX_FRAME_BYTES
        and (total_bytes - 2 - HEADER_BYTES) % 8 == 0
    ]
    if len(valid) != 1:
        raise ValueError("AU EFTEX length prefix does not describe a block-aligned frame")
    return valid[0]


def _apply_length_prefix(body: bytes, mode: LengthPrefixMode) -> bytes:
    if mode == "none":
        return body
    if mode == "u16_be_body":
        declared = len(body)
    elif mode == "u16_be_total":
        declared = len(body) + 2
    else:
        raise ValueError("AU EFTEX length prefix mode is invalid")
    if declared > 0xFFFF:
        raise ValueError("AU EFTEX length prefix exceeds 65,535 bytes")
    return declared.to_bytes(2, "big") + body


def _parse_header(header: bytes) -> bytes:
    if len(header) != HEADER_BYTES:
        raise ValueError("AU EFTEX header must be 39 bytes")
    if header[0:1] != b"T":
        raise ValueError("AU EFTEX header must start with T")
    expected = ((1, b"\xdf\x00", 1), (5, b"\xdf\x01", 8), (16, b"\xdf\x02", 6), (25, b"\xdf\x03", 10))
    for offset, tag, length in expected:
        if header[offset : offset + 2] != tag or header[offset + 2] != length:
            raise ValueError("AU EFTEX header TLV layout is invalid")
    if header[38:39] != b"B":
        raise ValueError("AU EFTEX encoding indicator must be B")
    return header[28:38]


def _data_iv(header: bytes) -> bytes:
    _parse_header(header)
    try:
        stan = header[19:25].decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError("AU EFTEX STAN must contain ASCII digits") from error
    if not stan.isdecimal():
        raise ValueError("AU EFTEX STAN must contain ASCII digits")
    packed_stan = bytes.fromhex(f"{int(stan):016d}")
    return bytes(value ^ mask for value, mask in zip(packed_stan, _DATA_IV_MASK))


def _display_value(name: str, value: object) -> str:
    if not isinstance(value, dict):
        raise ValueError(f"{name} must be a tagged value")
    if value.get("type") == "blob" and isinstance(value.get("value_base64"), str):
        try:
            size = len(base64.b64decode(value["value_base64"], validate=True))
        except (binascii.Error, ValueError) as error:
            raise ValueError(f"{name} contains invalid Base64") from error
        return f"[redacted blob: {size} bytes]"
    if value.get("type") in {"string", "int"} and isinstance(value.get("value"), str):
        text = value["value"]
        return f"[redacted: {len(text)} chars]" if name in _SENSITIVE_DISPLAY_FIELDS else text
    raise ValueError(f"{name} must be a tagged value")
