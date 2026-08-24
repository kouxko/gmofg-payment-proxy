"""Strict JSON-RPC dispatch for the AU EFTEX external package."""

from __future__ import annotations

import base64
import binascii
from collections.abc import Callable
from typing import Protocol

from .iso8583 import FIELD_SCHEMA


Document = dict[str, object]
JsonObject = dict[str, object]


class Codec(Protocol):
    """Crypto/codec boundary; JSON-RPC and Base64 never cross this interface."""

    def frame(self, direction: str, buffer: bytes) -> dict[str, object]: ...

    def decode(self, direction: str, frame: bytes) -> Document: ...

    def encode(self, direction: str, document: Document) -> bytes: ...

    def display(self, direction: str, document: Document) -> str: ...


_DIRECTIONS = ("upstream", "downstream")
_METHODS = {
    f"hooks.{direction}.split_frame": (direction, "frame")
    for direction in _DIRECTIONS
} | {
    f"hooks.{direction}.decrypt_message": (direction, "decode")
    for direction in _DIRECTIONS
} | {
    f"hooks.{direction}.encrypt_message": (direction, "encode")
    for direction in _DIRECTIONS
} | {
    f"document.{direction}.render_message": (direction, "display")
    for direction in _DIRECTIONS
}
RPC_METHODS = frozenset(_METHODS)

_FIELD_LABELS = {
    "message_type": "MTI",
    "primary_account_number": "DE2 Primary Account Number",
    "processing_code": "DE3 Processing Code",
    "amount": "DE4 Amount",
    "transmission_time": "DE7 Transmission Time",
    "stan": "DE11 STAN",
    "local_transaction_time": "DE12 Local Transaction Time",
    "expiration_date": "DE14 Expiration Date",
    "pos_data_code": "DE22 POS Data Code",
    "card_sequence_number": "DE23 Card Sequence Number",
    "function_code": "DE24 Function Code",
    "message_reason_code": "DE25 Message Reason Code",
    "reconciliation_date": "DE28 Reconciliation Date",
    "reconciliation_indicator": "DE29 Reconciliation Indicator",
    "track_2_data": "DE35 Track 2 Data",
    "retrieval_reference_number": "DE37 Retrieval Reference Number",
    "approval_code": "DE38 Approval Code",
    "action_code": "DE39 Action Code",
    "service_code": "DE40 Service Code",
    "terminal_id": "DE41 Terminal ID",
    "card_acceptor_id": "DE42 Card Acceptor ID",
    "amounts_fees": "DE46 Amounts, Fees",
    "additional_private": "DE48 Additional Private Data",
    "currency": "DE49 Currency",
    "reconciliation_currency": "DE50 Reconciliation Currency",
    "pin_data": "DE52 PIN Data",
    "security_control_information": "DE53 Security Control Information",
    "additional_amounts": "DE54 Additional Amounts",
    "icc_data": "DE55 ICC System Related Data",
    "original_data_elements": "DE56 Original Data Elements",
    "reserved_private": "DE63 Reserved, Private",
    "message_authentication_code": "DE64 Message Authentication Code",
    "credits_number": "DE74 Credits, Number",
    "credits_reversal_number": "DE75 Credits Reversal, Number",
    "debits_number": "DE76 Debits, Number",
    "debits_reversal_number": "DE77 Debits Reversal, Number",
    "authorisations_number": "DE81 Authorisations, Number",
    "credits_amount": "DE86 Credits, Amount",
    "credits_reversal_amount": "DE87 Credits Reversal, Amount",
    "debits_amount": "DE88 Debits, Amount",
    "debits_reversal_amount": "DE89 Debits Reversal, Amount",
    "authorisations_reversal_number": "DE90 Authorisations Reversal, Number",
    "net_reconciliation_amount": "DE97 Net Reconciliation Amount",
    "credits_fee_amounts": "DE109 Credits Fee Amounts",
    "debits_fee_amounts": "DE110 Debits Fee Amounts",
    "receipt_data": "DE123 Receipt Data",
    "display_data": "DE124 Display Data",
    "message_authentication_code_extended": "DE128 Message Authentication Code",
}
DOCUMENT_FIELD_NAMES = frozenset(field.name for field in FIELD_SCHEMA) | {
    "encoding_context"
}
_DOCUMENT_FIELDS = [
    {
        "name": field.name,
        "label": _FIELD_LABELS[field.name],
        "type": field.document_type,
    }
    for field in FIELD_SCHEMA
] + [
    {
        "name": "encoding_context",
        "label": "Authenticated Encoding Context",
        "type": "blob",
    }
]

REGISTRATION: JsonObject = {
    "api": 1,
    "package": {
        "id": "au-eftex",
        "name": "AU EFTEX",
        "version": "1.1.0",
        "description": "Python external package for AU EFTEX DUKPT protected messages",
    },
    "document": {
        direction: {
            "schema": {
                "id": f"au-eftex-{direction}",
                "title": f"AU EFTEX {direction.title()}",
                "version": 1,
                "fields": _DOCUMENT_FIELDS,
            },
            "display": "render_message",
        }
        for direction in _DIRECTIONS
    },
    "hooks": {
        direction: {
            "frame": "split_frame",
            "decode": "decrypt_message",
            "encode": "encrypt_message",
        }
        for direction in _DIRECTIONS
    },
}


class _InvalidParams(ValueError):
    pass


RpcDispatcher = Callable[[JsonObject], JsonObject]


def create_rpc_dispatcher(codec: Codec) -> RpcDispatcher:
    """Create connection-local state; registration count resets on reconnect."""

    registered = False

    def dispatch(request: JsonObject) -> JsonObject:
        nonlocal registered
        request_id = request.get("id")
        method = request.get("method")
        if not isinstance(method, str):
            return _failure(request_id, -32600, "invalid JSON-RPC request")

        if method == "package.register":
            if registered:
                return _failure(
                    request_id,
                    -32001,
                    "package.register may be called only once per connection",
                )
            try:
                _validate_registration(request.get("params"))
            except _InvalidParams as error:
                return _failure(request_id, -32002, str(error))
            registered = True
            return _success(request_id, REGISTRATION)

        if not registered:
            return _failure(request_id, -32003, "package.register must complete first")

        target = _METHODS.get(method)
        if target is None:
            return _failure(request_id, -32601, "method not found")

        direction, operation = target
        try:
            result = _dispatch_codec(codec, direction, operation, request.get("params"))
        except _InvalidParams as error:
            return _failure(request_id, -32002, str(error))
        except Exception as error:
            # Codec errors can contain keys, KSNs, plaintext, or vendor frames.
            code = _processing_error_code(operation, error)
            return _failure(
                request_id,
                -32002,
                f"external package processing failed [{code}]",
            )
        return _success(request_id, result)

    return dispatch


def _dispatch_codec(
    codec: Codec,
    direction: str,
    operation: str,
    params: object,
) -> object:
    if operation == "frame":
        value = _closed_object(params, ("buffer_base64",), "frame params")
        buffer = _decode_base64(value["buffer_base64"], "buffer_base64")
        return codec.frame(direction, buffer)
    if operation == "decode":
        value = _closed_object(params, ("frame_base64",), "decode params")
        frame = _decode_base64(value["frame_base64"], "frame_base64")
        return {"document": codec.decode(direction, frame)}
    if operation == "encode":
        value = _closed_object(params, ("document",), "encode params")
        document = _document(value["document"])
        return {"frame_base64": _encode_base64(codec.encode(direction, document))}

    value = _closed_object(params, ("document",), "display params")
    document = _document(value["document"])
    return {"html": codec.display(direction, document)}


def _validate_registration(params: object) -> None:
    value = _closed_object(params, ("api",), "package.register params")
    if value["api"] != 1 or isinstance(value["api"], bool):
        raise _InvalidParams("only external package API 1 is supported")


def _closed_object(value: object, keys: tuple[str, ...], name: str) -> JsonObject:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise _InvalidParams(f"{name} must be an object")
    if set(value) != set(keys) or len(value) != len(keys):
        raise _InvalidParams(f"{name} contains missing or unknown fields")
    return value


def _document(value: object) -> Document:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise _InvalidParams("document must be an object")
    return value


def _decode_base64(value: object, name: str) -> bytes:
    if not isinstance(value, str):
        raise _InvalidParams(f"{name} must be a canonical Base64 string")
    try:
        encoded = value.encode("ascii")
        decoded = base64.b64decode(encoded, validate=True)
    except (UnicodeEncodeError, binascii.Error, ValueError) as error:
        raise _InvalidParams(f"{name} must be a canonical Base64 string") from error
    if base64.b64encode(decoded) != encoded:
        raise _InvalidParams(f"{name} must be a canonical Base64 string")
    return decoded


def _encode_base64(value: bytes) -> str:
    if not isinstance(value, bytes):
        raise TypeError("codec encode must return bytes")
    return base64.b64encode(value).decode("ascii")


def _processing_error_code(operation: str, error: Exception) -> str:
    message = str(error).lower()
    if "replacement mac" in message:
        return "MAC_REPLACEMENT_REQUIRED"
    if "encoding context" in message:
        return "ENCODING_CONTEXT_INVALID"
    if "length prefix" in message:
        return "LENGTH_PREFIX_INVALID"
    if "data key direction" in message:
        return "DATA_KEY_DIRECTION_MISMATCH"
    if "unexpectedly not encrypted" in message:
        return "UNEXPECTED_PLAINTEXT_PAYLOAD"
    if "decrypted mti" in message:
        return "DECRYPTED_MTI_INVALID"
    if "padding" in message:
        return "PADDING_INVALID"
    if "header" in message or "encoding indicator" in message:
        return "HEADER_INVALID"
    if "ksn" in message or "transaction counter" in message:
        return "KSN_INVALID"
    if "iso 8583" in message or "bitmap" in message or "document field" in message:
        return "ISO8583_PARSE_FAILED" if operation in {"frame", "decode"} else "ISO8583_ENCODE_FAILED"
    return {
        "frame": "FRAME_FAILED",
        "decode": "DECODE_FAILED",
        "encode": "ENCODE_FAILED",
        "display": "DISPLAY_FAILED",
    }[operation]


def _success(request_id: object, result: object) -> JsonObject:
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def _failure(request_id: object, code: int, message: str) -> JsonObject:
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {"code": code, "message": message},
    }
