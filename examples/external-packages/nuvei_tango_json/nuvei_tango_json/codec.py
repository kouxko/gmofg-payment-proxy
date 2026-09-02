from __future__ import annotations

import base64
import binascii
import hashlib
import hmac
import html
import json
import secrets
from collections import OrderedDict
from copy import deepcopy
from typing import Literal


Direction = Literal["upstream", "downstream"]
LENGTH_BYTES = 4
CONTROL_BYTES = 4
SEQUENCE_BYTES = 8
MINIMUM_BODY_BYTES = CONTROL_BYTES + SEQUENCE_BYTES + 2
MAXIMUM_BODY_BYTES = 1024 * 1024 - LENGTH_BYTES
ENCODING_CONTEXT_FIELD = "encoding_context"
_CONTEXT_MAGIC = b"NTJ1"
_CONTEXT_TOKEN_BYTES = 32
_CONTEXT_TAG_BYTES = 32
_MAXIMUM_CONTEXTS = 4096
_MAXIMUM_CONTEXT_BYTES = 16 * 1024 * 1024
_DOCUMENT_FIELDS = frozenset(
    {
        "frame_length",
        "control_header",
        "sequence",
        "message_type",
        "json_preview",
        ENCODING_CONTEXT_FIELD,
    }
)
_SENSITIVE_KEY_PARTS = (
    "pan",
    "track2",
    "track1",
    "pin",
    "mac",
    "key",
    "ksn",
    "cryptogram",
    "iccrltddata",
)


class TangoJsonCodec:
    """Strict observer codec; encode only returns the original decoded frame."""

    def __init__(self, *, context_key: bytes | None = None) -> None:
        key = context_key if context_key is not None else secrets.token_bytes(32)
        if not isinstance(key, bytes) or len(key) != 32:
            raise ValueError("Nuvei Tango context key must be 32 bytes")
        self._context_key = bytes(key)
        self._contexts: OrderedDict[bytes, tuple[Direction, bytes, dict[str, object]]] = (
            OrderedDict()
        )
        self._context_bytes = 0

    def frame(self, direction: str, buffer: bytes) -> dict[str, object]:
        _direction(direction)
        if not isinstance(buffer, bytes):
            raise TypeError("Nuvei Tango frame buffer must be bytes")
        if len(buffer) < LENGTH_BYTES:
            return {"status": "need_more"}
        body_bytes = int.from_bytes(buffer[:LENGTH_BYTES], "big")
        _validate_declared_length(body_bytes)
        frame_bytes = LENGTH_BYTES + body_bytes
        if len(buffer) < frame_bytes:
            return {"status": "need_more"}
        return {"status": "complete", "consumed_bytes": frame_bytes}

    def decode(self, direction: str, frame: bytes) -> dict[str, object]:
        normalized = _direction(direction)
        control, sequence, message = _decode_frame(frame)
        message_type = next(iter(message))
        preview = json.dumps(
            _redact(message),
            ensure_ascii=False,
            indent=2,
            allow_nan=False,
        )
        public: dict[str, object] = {
            "frame_length": {
                "type": "int",
                "value": str(len(frame) - LENGTH_BYTES),
            },
            "control_header": {
                "type": "blob",
                "value_base64": base64.b64encode(control).decode("ascii"),
            },
            "sequence": {"type": "string", "value": sequence},
            "message_type": {"type": "string", "value": message_type},
            "json_preview": {"type": "string", "value": preview},
        }
        document = deepcopy(public)
        document[ENCODING_CONTEXT_FIELD] = self._store_context(normalized, frame, public)
        return document

    def encode(self, direction: str, document: dict[str, object]) -> bytes:
        normalized = _direction(direction)
        frame, expected = self._load_context(normalized, document)
        actual = {
            name: value
            for name, value in document.items()
            if name != ENCODING_CONTEXT_FIELD
        }
        if set(document) != _DOCUMENT_FIELDS or actual != expected:
            raise ValueError("Nuvei Tango read-only document was modified")
        return frame

    def display(self, direction: str, document: dict[str, object]) -> str:
        normalized = _direction(direction)
        _, expected = self._load_context(normalized, document)
        actual = {
            name: value
            for name, value in document.items()
            if name != ENCODING_CONTEXT_FIELD
        }
        if set(document) != _DOCUMENT_FIELDS or actual != expected:
            raise ValueError("Nuvei Tango read-only document was modified")
        label = "Upstream" if normalized == "upstream" else "Downstream"
        sequence = _tagged_string(expected["sequence"], "sequence")
        message_type = _tagged_string(expected["message_type"], "message_type")
        preview = _tagged_string(expected["json_preview"], "json_preview")
        return (
            '<section class="protocol-document"><h3>Nuvei Tango JSON</h3>'
            "<table><tbody>"
            f"<tr><th>Direction</th><td>{html.escape(label)}</td></tr>"
            f"<tr><th>Sequence</th><td>{html.escape(sequence)}</td></tr>"
            f"<tr><th>Message type</th><td>{html.escape(message_type)}</td></tr>"
            "</tbody></table>"
            f"<pre>{html.escape(preview)}</pre></section>"
        )

    def _store_context(
        self,
        direction: Direction,
        frame: bytes,
        public: dict[str, object],
    ) -> dict[str, str]:
        token = secrets.token_bytes(_CONTEXT_TOKEN_BYTES)
        tag = hmac.digest(
            self._context_key,
            _CONTEXT_MAGIC + direction.encode("ascii") + token,
            hashlib.sha256,
        )
        self._contexts[token] = (direction, bytes(frame), deepcopy(public))
        self._context_bytes += len(frame)
        self._contexts.move_to_end(token)
        while (
            len(self._contexts) > _MAXIMUM_CONTEXTS
            or self._context_bytes > _MAXIMUM_CONTEXT_BYTES
        ):
            _, (_, evicted_frame, _) = self._contexts.popitem(last=False)
            self._context_bytes -= len(evicted_frame)
        return {
            "type": "blob",
            "value_base64": base64.b64encode(_CONTEXT_MAGIC + token + tag).decode("ascii"),
        }

    def _load_context(
        self,
        direction: Direction,
        document: dict[str, object],
    ) -> tuple[bytes, dict[str, object]]:
        if not isinstance(document, dict):
            raise ValueError("Nuvei Tango encoding context requires a document object")
        value = document.get(ENCODING_CONTEXT_FIELD)
        if not isinstance(value, dict) or set(value) != {"type", "value_base64"}:
            raise ValueError("Nuvei Tango encoding context must be a canonical blob")
        encoded = value.get("value_base64")
        if value.get("type") != "blob" or not isinstance(encoded, str):
            raise ValueError("Nuvei Tango encoding context must be a canonical blob")
        try:
            raw = base64.b64decode(encoded, validate=True)
        except (UnicodeEncodeError, binascii.Error, ValueError) as error:
            raise ValueError("Nuvei Tango encoding context contains invalid Base64") from error
        if base64.b64encode(raw).decode("ascii") != encoded:
            raise ValueError("Nuvei Tango encoding context Base64 must be canonical")
        expected_bytes = len(_CONTEXT_MAGIC) + _CONTEXT_TOKEN_BYTES + _CONTEXT_TAG_BYTES
        if len(raw) != expected_bytes or not raw.startswith(_CONTEXT_MAGIC):
            raise ValueError("Nuvei Tango encoding context has an invalid envelope")
        token = raw[len(_CONTEXT_MAGIC) : len(_CONTEXT_MAGIC) + _CONTEXT_TOKEN_BYTES]
        supplied_tag = raw[-_CONTEXT_TAG_BYTES:]
        expected_tag = hmac.digest(
            self._context_key,
            _CONTEXT_MAGIC + direction.encode("ascii") + token,
            hashlib.sha256,
        )
        if not hmac.compare_digest(supplied_tag, expected_tag):
            raise ValueError("Nuvei Tango encoding context authentication failed")
        stored = self._contexts.get(token)
        if stored is None or stored[0] != direction:
            raise ValueError("Nuvei Tango encoding context is unavailable")
        self._contexts.move_to_end(token)
        return bytes(stored[1]), deepcopy(stored[2])


def _decode_frame(frame: bytes) -> tuple[bytes, str, dict[str, object]]:
    if not isinstance(frame, bytes):
        raise TypeError("Nuvei Tango frame must be bytes")
    if len(frame) < LENGTH_BYTES:
        raise ValueError("Nuvei Tango frame is missing its length prefix")
    declared = int.from_bytes(frame[:LENGTH_BYTES], "big")
    _validate_declared_length(declared)
    if len(frame) != LENGTH_BYTES + declared:
        raise ValueError("Nuvei Tango length prefix does not match the complete frame")
    body = frame[LENGTH_BYTES:]
    control = body[:CONTROL_BYTES]
    sequence_bytes = body[CONTROL_BYTES : CONTROL_BYTES + SEQUENCE_BYTES]
    if not sequence_bytes.isascii() or not sequence_bytes.isdigit():
        raise ValueError("Nuvei Tango sequence must contain exactly eight ASCII digits")
    try:
        json_text = body[CONTROL_BYTES + SEQUENCE_BYTES :].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("Nuvei Tango JSON payload must be UTF-8") from error
    try:
        message = json.loads(
            json_text,
            object_pairs_hook=_closed_json_object,
            parse_constant=_reject_json_constant,
        )
    except (TypeError, ValueError, json.JSONDecodeError) as error:
        raise ValueError("Nuvei Tango JSON payload is invalid") from error
    if not isinstance(message, dict) or len(message) != 1:
        raise ValueError("Nuvei Tango JSON payload must contain one top-level message object")
    message_type = next(iter(message))
    if not isinstance(message_type, str) or not message_type:
        raise ValueError("Nuvei Tango message type must be a non-empty string")
    return control, sequence_bytes.decode("ascii"), message


def _validate_declared_length(body_bytes: int) -> None:
    if body_bytes < MINIMUM_BODY_BYTES:
        raise ValueError("Nuvei Tango length prefix is smaller than the minimum frame")
    if body_bytes > MAXIMUM_BODY_BYTES:
        raise ValueError("Nuvei Tango length prefix exceeds the 1 MiB package limit")


def _closed_json_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("Nuvei Tango JSON payload contains a duplicate object key")
        value[key] = item
    return value


def _reject_json_constant(value: str) -> object:
    raise ValueError(f"Nuvei Tango JSON payload contains invalid constant {value}")


def _redact(value: object, key: str = "") -> object:
    normalized = "".join(character for character in key.casefold() if character.isalnum())
    if any(part in normalized for part in _SENSITIVE_KEY_PARTS):
        return "[redacted]"
    if isinstance(value, dict):
        return {item_key: _redact(item, item_key) for item_key, item in value.items()}
    if isinstance(value, list):
        return [_redact(item) for item in value]
    return value


def _direction(value: str) -> Direction:
    if value not in {"upstream", "downstream"}:
        raise ValueError("Nuvei Tango direction must be upstream or downstream")
    return value  # type: ignore[return-value]


def _tagged_string(value: object, name: str) -> str:
    if not isinstance(value, dict) or set(value) != {"type", "value"}:
        raise ValueError(f"Nuvei Tango {name} must be a tagged string")
    text = value.get("value")
    if value.get("type") != "string" or not isinstance(text, str):
        raise ValueError(f"Nuvei Tango {name} must be a tagged string")
    return text
