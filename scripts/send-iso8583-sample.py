#!/usr/bin/env python3
"""Send the built-in ISO 8583 financial request to 127.0.0.1:8080."""

from __future__ import annotations

import json
import socket
from argparse import ArgumentParser
from pathlib import Path


DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8080
TIMEOUT_SECONDS = 3
SAMPLE_PATH = (
    Path(__file__).resolve().parent.parent
    / "templates/socket-protocol/iso8583-standard/samples/financial-request.json"
)


def receive_exact(connection: socket.socket, size: int) -> bytes:
    chunks: list[bytes] = []
    remaining = size
    while remaining:
        chunk = connection.recv(remaining)
        if not chunk:
            raise ConnectionError("连接在完整响应返回前已关闭")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def parse_target() -> tuple[str, int]:
    parser = ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    arguments = parser.parse_args()
    return arguments.host, arguments.port


def main() -> None:
    host, port = parse_target()
    sample = json.loads(SAMPLE_PATH.read_text(encoding="utf-8"))
    request = bytes.fromhex(sample["complete_frame_hex"])

    with socket.create_connection((host, port), timeout=TIMEOUT_SECONDS) as connection:
        connection.settimeout(TIMEOUT_SECONDS)
        connection.sendall(request)
        print(f"sent_bytes={len(request)}")
        print(f"sent_hex={request.hex()}")
        try:
            response_prefix = receive_exact(connection, 2)
            response_length = int.from_bytes(response_prefix, "big")
            response_payload = receive_exact(connection, response_length)
        except (ConnectionError, TimeoutError) as error:
            raise SystemExit(f"response_error={error}") from None

    print(f"received_bytes={len(response_prefix) + len(response_payload)}")
    print(f"received_hex={(response_prefix + response_payload).hex()}")
    print(f"received_payload={response_payload.decode('ascii')}")


if __name__ == "__main__":
    main()
