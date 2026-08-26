from __future__ import annotations

import base64
import binascii
from collections.abc import Callable
from typing import Protocol


Document = dict[str, object]
JsonObject = dict[str, object]


class Codec(Protocol):
    def frame(self, direction: str, buffer: bytes) -> dict[str, object]: ...

    def decode(self, direction: str, frame: bytes) -> Document: ...

    def encode(self, direction: str, document: Document) -> bytes: ...

    def display(self, direction: str, document: Document) -> str: ...


_DIRECTIONS = ("upstream", "downstream")
_METHODS = {
    f"hooks.{direction}.split_frame": (direction, "frame") for direction in _DIRECTIONS
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
_FIELDS = [
    {"name": "frame_length", "label": "Declared Body Bytes", "type": "int"},
    {"name": "control_header", "label": "Opaque Control Header", "type": "blob"},
    {"name": "sequence", "label": "Sequence", "type": "string"},
    {"name": "message_type", "label": "Message Type", "type": "string"},
    {"name": "json_preview", "label": "Redacted JSON Preview", "type": "string"},
    {"name": "encoding_context", "label": "Read-only Encoding Context", "type": "blob"},
]
REGISTRATION: JsonObject = {
    "api": 1,
    "package": {
        "id": "nuvei-tango-json",
        "name": "Nuvei Tango JSON",
        "version": "1.0.0",
        "description": "Python read-only parser for length-prefixed Nuvei Tango JSON messages",
    },
    "document": {
        direction: {
            "schema": {
                "id": f"nuvei-tango-json-{direction}",
                "title": f"Nuvei Tango JSON {direction.title()}",
                "version": 1,
                "fields": _FIELDS,
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
                params = _closed_object(request.get("params"), ("api",), "package.register params")
                if params["api"] != 1 or isinstance(params["api"], bool):
                    raise _InvalidParams("only external package API 1 is supported")
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
        except Exception:
            return _failure(
                request_id,
                -32002,
                f"external package processing failed [{_error_code(operation)}]",
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
        return codec.frame(direction, _decode_base64(value["buffer_base64"], "buffer_base64"))
    if operation == "decode":
        value = _closed_object(params, ("frame_base64",), "decode params")
        return {
            "document": codec.decode(
                direction,
                _decode_base64(value["frame_base64"], "frame_base64"),
            )
        }
    if operation == "encode":
        value = _closed_object(params, ("document",), "encode params")
        document = value["document"]
        if not isinstance(document, dict):
            raise _InvalidParams("document must be an object")
        return {"frame_base64": _encode_base64(codec.encode(direction, document))}
    value = _closed_object(params, ("document",), "display params")
    document = value["document"]
    if not isinstance(document, dict):
        raise _InvalidParams("document must be an object")
    return {"html": codec.display(direction, document)}


def _closed_object(value: object, keys: tuple[str, ...], name: str) -> JsonObject:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise _InvalidParams(f"{name} must be an object")
    if set(value) != set(keys) or len(value) != len(keys):
        raise _InvalidParams(f"{name} contains missing or unknown fields")
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


def _error_code(operation: str) -> str:
    return {
        "frame": "FRAME_FAILED",
        "decode": "DECODE_FAILED",
        "encode": "READ_ONLY_ENCODE_FAILED",
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
