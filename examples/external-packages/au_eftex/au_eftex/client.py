"""Reconnectable, bounded WebSocket peer for external-package JSON-RPC."""

from __future__ import annotations

import asyncio
import base64
import binascii
import datetime as dt
import hashlib
import ipaddress
import json
from collections.abc import Awaitable, Callable
from typing import Protocol
from urllib.parse import urlsplit

from .rpc import DOCUMENT_FIELD_NAMES, RPC_METHODS, Codec, create_rpc_dispatcher


MAX_WIRE_MESSAGE_BYTES = 1024 * 1024


class WebSocketPeer(Protocol):
    async def recv(self) -> str | bytes: ...

    async def send(self, message: str) -> None: ...

    async def close(self, code: int = 1000, reason: str = "") -> None: ...


Connector = Callable[[str, int], Awaitable[WebSocketPeer]]
Logger = Callable[[dict[str, object]], None]


class ExternalPackageClient:
    """Long-running peer; Proxy initiates registration and Ping/Pong control."""

    def __init__(
        self,
        *,
        url: str,
        codec: Codec,
        reconnect_delay: float = 1.0,
        allow_insecure_remote_ws: bool = False,
        connector: Connector | None = None,
        logger: Logger | None = None,
    ) -> None:
        parsed = urlsplit(url)
        local_plaintext = parsed.scheme == "ws" and _is_loopback_host(parsed.hostname)
        if (
            parsed.scheme not in {"ws", "wss"}
            or parsed.hostname is None
            or parsed.scheme == "ws"
            and not local_plaintext
            and not allow_insecure_remote_ws
            or parsed.path != "/packages"
            or parsed.username is not None
            or parsed.password is not None
            or bool(parsed.query)
            or bool(parsed.fragment)
        ):
            raise ValueError(
                "external package URL must use loopback ws or wss and the exact /packages path"
            )
        if reconnect_delay < 0:
            raise ValueError("reconnect_delay must be non-negative")

        self._url = url
        self._codec = codec
        self._reconnect_delay = reconnect_delay
        self._connector = connector or _connect
        self._logger = logger or _print_log
        self._active_socket: WebSocketPeer | None = None
        self._stopped = True
        self._stop_event = asyncio.Event()

    async def run(self) -> None:
        self._stopped = False
        self._stop_event.clear()
        attempt = 0
        while not self._stopped:
            attempt += 1
            self._log("connection_attempt", attempt=attempt)
            socket: WebSocketPeer | None = None
            try:
                socket = await self._connector(self._url, MAX_WIRE_MESSAGE_BYTES)
                self._active_socket = socket
                self._log("connected", attempt=attempt)
                await self._serve_connection(socket)
            except asyncio.CancelledError:
                raise
            except Exception as error:
                if not self._stopped:
                    self._log(
                        "connection_error",
                        attempt=attempt,
                        error_type=type(error).__name__,
                    )
            finally:
                if self._active_socket is socket:
                    self._active_socket = None
                if socket is not None:
                    self._log("disconnected", attempt=attempt)

            if not self._stopped:
                await self._wait_reconnect_delay()

    async def stop(self) -> None:
        self._stopped = True
        self._stop_event.set()
        socket = self._active_socket
        if socket is not None:
            await socket.close(1000, "external package stopped")

    async def _serve_connection(self, socket: WebSocketPeer) -> None:
        dispatch = create_rpc_dispatcher(self._codec)
        while not self._stopped:
            message = await socket.recv()
            if not isinstance(message, str):
                self._log("ignored_non_text_message", message_type=type(message).__name__)
                continue
            if len(message.encode("utf-8")) > MAX_WIRE_MESSAGE_BYTES:
                self._log("wire_message_rejected", reason="too_large")
                await socket.close(1009, "JSON-RPC message exceeds 1 MiB")
                return

            try:
                request = _parse_request(message)
            except (ValueError, TypeError, json.JSONDecodeError) as error:
                self._log("protocol_error", error_type=type(error).__name__)
                await socket.close(1002, "invalid JSON-RPC request")
                return

            started_at = asyncio.get_running_loop().time()
            request_metadata = _safe_request_metadata(request)
            self._log("rpc_started", **request_metadata)
            response = dispatch(request)
            encoded = json.dumps(
                response,
                ensure_ascii=False,
                separators=(",", ":"),
                allow_nan=False,
            )
            if len(encoded.encode("utf-8")) > MAX_WIRE_MESSAGE_BYTES:
                self._log("wire_response_rejected", reason="too_large")
                await socket.close(1009, "JSON-RPC response exceeds 1 MiB")
                return

            await socket.send(encoded)
            self._log(
                "rpc_completed",
                **request_metadata,
                id_type=type(request["id"]).__name__,
                outcome="error" if "error" in response else "ok",
                **_safe_response_metadata(response),
                duration_ms=round(
                    (asyncio.get_running_loop().time() - started_at) * 1000
                ),
            )

    async def _wait_reconnect_delay(self) -> None:
        try:
            await asyncio.wait_for(
                self._stop_event.wait(),
                timeout=self._reconnect_delay,
            )
        except TimeoutError:
            pass

    def _log(self, event: str, **details: object) -> None:
        self._logger(
            {
                "timestamp": dt.datetime.now(dt.UTC).isoformat().replace("+00:00", "Z"),
                "level": "info",
                "event": event,
                **details,
            }
        )


async def _connect(url: str, max_size: int) -> WebSocketPeer:
    from websockets.asyncio.client import connect

    return await connect(url, compression=None, max_size=max_size)


def _parse_request(message: str) -> dict[str, object]:
    def reject_constant(value: str) -> object:
        raise ValueError(f"invalid JSON constant: {value}")

    def closed_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON object key")
            result[key] = value
        return result

    value = json.loads(
        message,
        object_pairs_hook=closed_object,
        parse_constant=reject_constant,
    )
    if not isinstance(value, dict) or set(value) != {"jsonrpc", "id", "method", "params"}:
        raise ValueError("JSON-RPC request contains missing or unknown fields")
    if value["jsonrpc"] != "2.0" or not isinstance(value["method"], str):
        raise ValueError("unsupported JSON-RPC request")
    request_id = value["id"]
    if isinstance(request_id, bool) or not isinstance(request_id, (str, int)):
        raise ValueError("JSON-RPC id must be a string or integer")
    if isinstance(request_id, int) and abs(request_id) > 2**53 - 1:
        raise ValueError("numeric JSON-RPC id must be a safe integer")
    return value


def _safe_request_metadata(request: dict[str, object]) -> dict[str, object]:
    method = request["method"]
    assert isinstance(method, str)
    parts = method.split(".")
    known_method = method if method == "package.register" or method in RPC_METHODS else "unknown"
    metadata: dict[str, object] = {
        "request_correlation": _correlation_token(request["id"]),
        "method": known_method,
    }
    if len(parts) == 3 and known_method != "unknown":
        metadata["direction"] = parts[1]
        metadata["operation"] = parts[2]
    params = request.get("params")
    if isinstance(params, dict):
        for name in ("buffer_base64", "frame_base64"):
            encoded = params.get(name)
            if isinstance(encoded, str):
                decoded_bytes = _canonical_base64_size(encoded)
                if decoded_bytes is not None:
                    metadata["input_bytes"] = decoded_bytes
        document = params.get("document")
        if isinstance(document, dict):
            metadata["document_field_count"] = len(document)
    return metadata


def _safe_response_metadata(response: dict[str, object]) -> dict[str, object]:
    error = response.get("error")
    if isinstance(error, dict) and isinstance(error.get("message"), str):
        message = error["message"]
        start = message.rfind("[")
        if start >= 0 and message.endswith("]"):
            return {"error_code": message[start + 1 : -1]}
        return {"error_code": "JSON_RPC_ERROR"}
    result = response.get("result")
    if not isinstance(result, dict):
        return {}
    metadata: dict[str, object] = {}
    status = result.get("status")
    if status in {"need_more", "complete"}:
        metadata["frame_status"] = status
        consumed = result.get("consumed_bytes")
        if isinstance(consumed, int):
            metadata["consumed_bytes"] = consumed
    encoded = result.get("frame_base64")
    if isinstance(encoded, str):
        size = _canonical_base64_size(encoded)
        if size is not None:
            metadata["output_bytes"] = size
    document_wrapper = result.get("document")
    if isinstance(document_wrapper, dict):
        fields = sorted(name for name in document_wrapper if name in DOCUMENT_FIELD_NAMES)
        if fields:
            metadata["decoded_field_count"] = len(fields)
            metadata["decoded_fields"] = fields
    html_value = result.get("html")
    if isinstance(html_value, str):
        metadata["display_bytes"] = len(html_value.encode("utf-8"))
    return metadata


def _canonical_base64_size(value: str) -> int | None:
    try:
        encoded = value.encode("ascii")
        decoded = base64.b64decode(encoded, validate=True)
    except (UnicodeEncodeError, binascii.Error, ValueError):
        return None
    return len(decoded) if base64.b64encode(decoded) == encoded else None


def _correlation_token(value: object) -> str:
    material = f"{type(value).__name__}:{value}".encode("utf-8", errors="replace")
    return hashlib.sha256(material).hexdigest()[:16]


def _is_loopback_host(hostname: str | None) -> bool:
    if hostname is None:
        return False
    if hostname.lower() == "localhost":
        return True
    try:
        return ipaddress.ip_address(hostname).is_loopback
    except ValueError:
        return False


def _print_log(event: dict[str, object]) -> None:
    print(json.dumps(event, ensure_ascii=False, separators=(",", ":"), allow_nan=False))
