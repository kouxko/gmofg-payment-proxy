from __future__ import annotations

import asyncio
import base64
import binascii
import datetime as dt
import json
from collections.abc import Callable
from urllib.parse import urlsplit

from .rpc import RPC_METHODS, Codec, create_rpc_dispatcher


MAX_WIRE_MESSAGE_BYTES = 1024 * 1024
Logger = Callable[[dict[str, object]], None]


class ExternalPackageClient:
    def __init__(
        self,
        *,
        url: str,
        codec: Codec,
        reconnect_delay: float = 1.0,
        allow_insecure_remote_ws: bool = False,
        logger: Logger | None = None,
    ) -> None:
        parsed = urlsplit(url)
        local = parsed.hostname in {"127.0.0.1", "::1", "localhost"}
        if (
            parsed.scheme not in {"ws", "wss"}
            or parsed.hostname is None
            or parsed.path != "/packages"
            or parsed.username is not None
            or parsed.password is not None
            or parsed.query
            or parsed.fragment
            or parsed.scheme == "ws" and not local and not allow_insecure_remote_ws
        ):
            raise ValueError(
                "external package URL must use loopback ws or wss and the exact /packages path"
            )
        if reconnect_delay < 0:
            raise ValueError("reconnect delay must be non-negative")
        self._url = url
        self._codec = codec
        self._reconnect_delay = reconnect_delay
        self._logger = logger or _print_log
        self._stopped = True
        self._active_socket: object | None = None

    async def run(self) -> None:
        from websockets.asyncio.client import connect

        self._stopped = False
        attempt = 0
        while not self._stopped:
            attempt += 1
            self._log("connection_attempt", attempt=attempt)
            try:
                async with connect(
                    self._url,
                    compression=None,
                    max_size=MAX_WIRE_MESSAGE_BYTES,
                ) as socket:
                    self._active_socket = socket
                    self._log("connected", attempt=attempt)
                    await self._serve(socket)
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
                self._active_socket = None
                self._log("disconnected", attempt=attempt)
            if not self._stopped:
                await asyncio.sleep(self._reconnect_delay)

    async def stop(self) -> None:
        self._stopped = True
        socket = self._active_socket
        if socket is not None:
            await socket.close(1000, "external package stopped")  # type: ignore[attr-defined]

    async def _serve(self, socket: object) -> None:
        dispatch = create_rpc_dispatcher(self._codec)
        async for message in socket:  # type: ignore[attr-defined]
            if not isinstance(message, str):
                self._log("ignored_non_text_message", message_type=type(message).__name__)
                continue
            try:
                request = _parse_request(message)
            except (TypeError, ValueError, json.JSONDecodeError) as error:
                self._log("protocol_error", error_type=type(error).__name__)
                await socket.close(1002, "invalid JSON-RPC request")  # type: ignore[attr-defined]
                return
            metadata = _safe_request_metadata(request)
            self._log("rpc_started", **metadata)
            started_at = asyncio.get_running_loop().time()
            response = dispatch(request)
            encoded = json.dumps(response, ensure_ascii=False, separators=(",", ":"), allow_nan=False)
            if len(encoded.encode("utf-8")) > MAX_WIRE_MESSAGE_BYTES:
                self._log("wire_response_rejected", reason="too_large", **metadata)
                await socket.close(1009, "JSON-RPC response exceeds 1 MiB")  # type: ignore[attr-defined]
                return
            await socket.send(encoded)  # type: ignore[attr-defined]
            self._log(
                "rpc_completed",
                **metadata,
                **_safe_response_metadata(response),
                duration_ms=round(
                    (asyncio.get_running_loop().time() - started_at) * 1000
                ),
            )

    def _log(self, event: str, **details: object) -> None:
        self._logger({"event": event, **details})


def _parse_request(message: str) -> dict[str, object]:
    if len(message.encode("utf-8")) > MAX_WIRE_MESSAGE_BYTES:
        raise ValueError("JSON-RPC request exceeds 1 MiB")

    def closed_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON-RPC key")
            result[key] = value
        return result

    value = json.loads(message, object_pairs_hook=closed_object)
    if not isinstance(value, dict) or set(value) != {"jsonrpc", "id", "method", "params"}:
        raise ValueError("invalid JSON-RPC request")
    if value["jsonrpc"] != "2.0" or not isinstance(value["method"], str):
        raise ValueError("invalid JSON-RPC request")
    return value


def _safe_request_metadata(request: dict[str, object]) -> dict[str, object]:
    method = request["method"]
    assert isinstance(method, str)
    known_method = method if method == "package.register" or method in RPC_METHODS else "unknown"
    metadata: dict[str, object] = {
        "method": known_method,
        "id_type": type(request["id"]).__name__,
    }
    if method == "package.register":
        metadata["operation"] = "register"
        return metadata
    parts = method.split(".")
    if method in RPC_METHODS and len(parts) == 3:
        metadata["direction"] = parts[1]
        metadata["operation"] = {
            "split_frame": "frame",
            "decrypt_message": "decode",
            "encrypt_message": "encode",
            "render_message": "display",
        }[parts[2]]
    params = request.get("params")
    if not isinstance(params, dict):
        return metadata
    for field in ("buffer_base64", "frame_base64"):
        value = params.get(field)
        byte_count = _canonical_base64_length(value)
        if byte_count is not None:
            metadata["input_bytes"] = byte_count
            break
    document = params.get("document")
    if isinstance(document, dict):
        metadata["document_field_count"] = len(document)
    return metadata


def _safe_response_metadata(response: dict[str, object]) -> dict[str, object]:
    error = response.get("error")
    if isinstance(error, dict):
        metadata: dict[str, object] = {"outcome": "error"}
        code = error.get("code")
        if isinstance(code, int) and not isinstance(code, bool):
            metadata["jsonrpc_error_code"] = code
        message = error.get("message")
        if isinstance(message, str):
            match = message.rsplit("[", 1)
            if len(match) == 2 and match[1].endswith("]"):
                stable = match[1][:-1]
                if stable and all(character.isupper() or character == "_" for character in stable):
                    metadata["error_code"] = stable
        return metadata
    metadata = {"outcome": "ok"}
    result = response.get("result")
    if not isinstance(result, dict):
        return metadata
    status = result.get("status")
    if status in {"need_more", "complete"}:
        metadata["frame_status"] = status
    consumed = result.get("consumed_bytes")
    if isinstance(consumed, int) and not isinstance(consumed, bool):
        metadata["consumed_bytes"] = consumed
    document = result.get("document")
    if isinstance(document, dict):
        metadata["document_field_count"] = len(document)
    output_bytes = _canonical_base64_length(result.get("frame_base64"))
    if output_bytes is not None:
        metadata["output_bytes"] = output_bytes
    html_value = result.get("html")
    if isinstance(html_value, str):
        metadata["html_bytes"] = len(html_value.encode("utf-8"))
    package = result.get("package")
    if isinstance(package, dict):
        package_id = package.get("id")
        version = package.get("version")
        if isinstance(package_id, str):
            metadata["package_id"] = package_id
        if isinstance(version, str):
            metadata["package_version"] = version
    return metadata


def _canonical_base64_length(value: object) -> int | None:
    if not isinstance(value, str):
        return None
    try:
        encoded = value.encode("ascii")
        decoded = base64.b64decode(encoded, validate=True)
    except (UnicodeEncodeError, binascii.Error, ValueError):
        return None
    if base64.b64encode(decoded) != encoded:
        return None
    return len(decoded)


def _print_log(event: dict[str, object]) -> None:
    value = {
        "timestamp": dt.datetime.now(dt.UTC).isoformat().replace("+00:00", "Z"),
        "level": "info",
        **event,
    }
    print(json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False))
