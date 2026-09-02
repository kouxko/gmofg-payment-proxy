#!/usr/bin/env python3
import json
import socket
import threading
from datetime import datetime, timezone


AU_REQUEST = bytes.fromhex(
    "54df000132df01083132333435363738df0206303030303031df030affff9876543210e00008427b758dda6a29d38b8020b31687b21d636dbc15e6f3a17cdee8a868124d4c8f84"
)
AU_RESPONSE = bytes.fromhex(
    "54df000132df01083132333435363738df0206303030303031df030affff9876543210e000084247737e0317a4310697a84e728f754c84798309ef10edd18e"
)
NUVEI_JSON_REQUEST = bytes.fromhex(
    "0000002c0100010030303030303032307b224163637074724175746873746e526571223a7b2276616c7565223a317d7d"
)
NUVEI_JSON_RESPONSE = bytes.fromhex(
    "0000002d0100010030303030303032307b224163637074724175746873746e5273706e223a7b2276616c7565223a327d7d"
)


def nuvei_frame(path: str) -> bytes:
    with open(path, "rb") as source:
        payload = source.read()
    body = bytes.fromhex("01000100") + b"00000020" + payload
    return len(body).to_bytes(4, "big") + body


def receive_frame(connection: socket.socket) -> bytes:
    connection.settimeout(0.35)
    chunks = []
    while True:
        try:
            chunk = connection.recv(65536)
        except socket.timeout:
            break
        if not chunk:
            break
        chunks.append(chunk)
        data = b"".join(chunks)
        if len(data) >= 4 and data[0] == 0 and data[1] not in (0, 1):
            expected = 2 + int.from_bytes(data[:2], "big")
            if len(data) >= expected:
                break
        if len(data) >= 4 and data[:4] != b"T\xdf\x00\x01":
            expected = 4 + int.from_bytes(data[:4], "big")
            if 4 <= expected <= 1_048_576 and len(data) >= expected:
                break
        if data.startswith(b"T\xdf\x00\x01") and len(data) >= len(AU_REQUEST):
            break
        if len(data) == 6:
            break
    return b"".join(chunks)


def serve(port: int, rhai_request: bytes, rhai_response: bytes) -> None:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("0.0.0.0", port))
    listener.listen(16)
    print(json.dumps({"event": "listening", "port": port}), flush=True)
    while True:
        connection, peer = listener.accept()
        with connection:
            request = receive_frame(connection)
            if not request:
                print(json.dumps({"event": "probe", "port": port, "peer": peer[0]}), flush=True)
                continue
            if port == 18086 and request == AU_REQUEST:
                response = AU_RESPONSE
            elif port == 18087 and request == NUVEI_JSON_REQUEST:
                response = NUVEI_JSON_RESPONSE
            elif port == 18088 and request == rhai_request:
                response = rhai_response
            else:
                response = request
            connection.sendall(response)
            print(
                json.dumps(
                    {
                        "event": "exchange",
                        "timestamp": datetime.now(timezone.utc).isoformat(),
                        "port": port,
                        "peer": peer[0],
                        "request_hex": request.hex(),
                        "response_hex": response.hex(),
                    }
                ),
                flush=True,
            )


def main() -> None:
    base = __file__.rsplit("/replay/", 1)[0] + "/resources/"
    rhai_request = nuvei_frame(base + "nuvei-rhai-request.json")
    rhai_response = nuvei_frame(base + "nuvei-rhai-response.json")
    threads = [
        threading.Thread(target=serve, args=(port, rhai_request, rhai_response), daemon=True)
        for port in (18084, 18085, 18086, 18087, 18088)
    ]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()


if __name__ == "__main__":
    main()
