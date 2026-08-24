"""Real-loopback Socket cases shared by the desktop App acceptance harness."""

from __future__ import annotations

import queue
import socket
import threading
import time
from dataclasses import dataclass


ISO8583_SAMPLE_HEX = (
    "003930323030322000000080800030303030303030303030303030303130303030"
    "3831333134333035393132333435365445524d30303031333932"
)


class AcceptanceError(RuntimeError):
    """A stable, user-actionable acceptance failure."""


@dataclass(frozen=True)
class Iso8583Sample:
    message_type: str
    amount: int


def parse_iso8583_sample(frame: bytes) -> Iso8583Sample:
    """Parse the fixed built-in financial sample fields used by this E2E test."""

    _validate_sample_frame(frame)
    payload = frame[2:]
    return Iso8583Sample(
        message_type=payload[0:4].decode("ascii"),
        amount=int(payload[18:30].decode("ascii")),
    )


def with_iso8583_amount(frame: bytes, amount: int) -> bytes:
    _validate_sample_frame(frame)
    if not 0 <= amount <= 999_999_999_999:
        raise ValueError("ISO8583 sample amount must fit DE4")
    result = bytearray(frame)
    result[20:32] = f"{amount:012d}".encode("ascii")
    return bytes(result)


def with_iso8583_message_type(frame: bytes, message_type: str) -> bytes:
    _validate_sample_frame(frame)
    if len(message_type) != 4 or not message_type.isascii() or not message_type.isdigit():
        raise ValueError("ISO8583 message type must contain four ASCII digits")
    result = bytearray(frame)
    result[2:6] = message_type.encode("ascii")
    return bytes(result)


def run_scripted_socket_case(
    *,
    proxy_port: int,
    server_port: int,
    timeout_seconds: float,
) -> dict[str, object]:
    received: queue.Queue[bytes | BaseException] = queue.Queue()
    ready = threading.Event()

    def serve() -> None:
        try:
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", server_port))
                listener.listen(1)
                listener.settimeout(timeout_seconds)
                ready.set()
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(timeout_seconds)
                    request = _receive_frame(connection)
                    received.put(request)
                    response = with_iso8583_message_type(request, "0210")
                    _send_fragmented(connection, response)
        except BaseException as error:
            received.put(error)
            ready.set()

    server_thread = threading.Thread(target=serve, name="e2e-socket-server", daemon=True)
    server_thread.start()
    if not ready.wait(timeout_seconds):
        raise AcceptanceError("Socket mock Server did not become ready in time")
    request = bytes.fromhex(ISO8583_SAMPLE_HEX)
    try:
        with socket.create_connection(("127.0.0.1", proxy_port), timeout_seconds) as app:
            app.settimeout(timeout_seconds)
            _send_fragmented(app, request)
            response = _receive_frame(app)
    except ConnectionRefusedError as error:
        raise AcceptanceError(
            f"Socket proxy 127.0.0.1:{proxy_port} is not running; start the scripted listener"
        ) from error
    server_thread.join(timeout=timeout_seconds)
    server_result = received.get(timeout=timeout_seconds)
    if isinstance(server_result, BaseException):
        raise AcceptanceError(f"Socket mock Server failed: {server_result}") from server_result
    server_message = parse_iso8583_sample(server_result)
    app_message = parse_iso8583_sample(response)
    _require(server_message.message_type == "0200", "Socket Server parsed MTI 0200")
    _require(server_message.amount == 2222, "Socket upstream rules changed amount to 2222")
    _require(app_message.message_type == "0210", "Socket App parsed MTI 0210")
    _require(app_message.amount == 4444, "Socket downstream rules changed amount to 4444")
    return {"server_received_amount": 2222, "app_received_amount": 4444, "mti": "0210"}


def run_raw_transparent_case(
    *,
    proxy_port: int,
    server_port: int,
    timeout_seconds: float,
) -> dict[str, object]:
    """Prove Direct mode preserves arbitrary bytes and half-close semantics."""

    request = bytes((index * 197) & 0xFF for index in range(32_771))
    response = bytes((index * 89) & 0xFF for index in range(16_387))
    received: queue.Queue[bytes | BaseException] = queue.Queue()
    ready = threading.Event()

    def serve() -> None:
        try:
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", server_port))
                listener.listen(1)
                listener.settimeout(timeout_seconds)
                ready.set()
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(timeout_seconds)
                    upstream = _receive_until_eof(connection)
                    received.put(upstream)
                    _send_chunks(connection, response, 257)
                    connection.shutdown(socket.SHUT_WR)
        except BaseException as error:
            received.put(error)
            ready.set()

    server_thread = threading.Thread(target=serve, name="e2e-raw-server", daemon=True)
    server_thread.start()
    if not ready.wait(timeout_seconds):
        raise AcceptanceError("Raw Socket mock Server did not become ready in time")
    try:
        with socket.create_connection(("127.0.0.1", proxy_port), timeout_seconds) as app:
            app.settimeout(timeout_seconds)
            _send_chunks(app, request, 193)
            app.shutdown(socket.SHUT_WR)
            actual_response = _receive_until_eof(app)
    except ConnectionRefusedError as error:
        raise AcceptanceError(
            f"Socket proxy 127.0.0.1:{proxy_port} is not running; start the raw listener"
        ) from error
    server_thread.join(timeout=timeout_seconds)
    server_result = received.get(timeout=timeout_seconds)
    if isinstance(server_result, BaseException):
        raise AcceptanceError(f"Raw Socket mock Server failed: {server_result}") from server_result
    _require(server_result == request, "Raw transparent App bytes reached Server unchanged")
    _require(actual_response == response, "Raw transparent Server bytes reached App unchanged")
    return {
        "app_to_server_bytes": len(request),
        "server_to_app_bytes": len(response),
        "half_close_preserved": True,
    }


def _validate_sample_frame(frame: bytes) -> None:
    if len(frame) < 32:
        raise AcceptanceError("ISO8583 frame is shorter than the fixed test profile")
    declared = int.from_bytes(frame[:2], "big")
    if declared != len(frame) - 2:
        raise AcceptanceError("ISO8583 frame length prefix does not match the payload")


def _receive_exact(connection: socket.socket, size: int) -> bytes:
    result = bytearray()
    while len(result) < size:
        chunk = connection.recv(size - len(result))
        if not chunk:
            raise AcceptanceError("Socket closed before a complete frame was received")
        result.extend(chunk)
    return bytes(result)


def _receive_frame(connection: socket.socket) -> bytes:
    prefix = _receive_exact(connection, 2)
    return prefix + _receive_exact(connection, int.from_bytes(prefix, "big"))


def _receive_until_eof(connection: socket.socket) -> bytes:
    result = bytearray()
    while True:
        chunk = connection.recv(16_384)
        if not chunk:
            return bytes(result)
        result.extend(chunk)


def _send_fragmented(connection: socket.socket, frame: bytes) -> None:
    start = 0
    for end in (9, 31, len(frame)):
        connection.sendall(frame[start:end])
        start = end
        time.sleep(0.02)


def _send_chunks(connection: socket.socket, payload: bytes, chunk_size: int) -> None:
    for start in range(0, len(payload), chunk_size):
        connection.sendall(payload[start : start + chunk_size])


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AcceptanceError(f"FAILED: {message}")
    print(f"PASS  {message}")
