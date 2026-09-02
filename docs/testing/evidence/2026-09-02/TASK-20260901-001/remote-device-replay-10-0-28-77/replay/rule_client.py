#!/usr/bin/env python3
import json
import socket
from pathlib import Path


REMOTE = "10.0.28.77"
BASE = Path(__file__).resolve().parent.parent

AU_REQUEST = bytes.fromhex(
    "54df000132df01083132333435363738df0206303030303031df030affff9876543210e00008427b758dda6a29d38b8020b31687b21d636dbc15e6f3a17cdee8a868124d4c8f84"
)
AU_RESPONSE = bytes.fromhex(
    "54df000132df01083132333435363738df0206303030303031df030affff9876543210e000084247737e0317a4310697a84e728f754c84798309ef10edd18e"
)
ISO_STANDARD_HIT = bytes.fromhex("000430323030")
ISO_STANDARD_CHANGED = bytes.fromhex("000430313030")
ISO_STANDARD_MISS = bytes.fromhex("000430343030")
ISO_DENO_HIT = bytes.fromhex(
    "0039303230303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932"
)
ISO_DENO_CHANGED = bytes.fromhex(
    "0039303130303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932"
)
ISO_DENO_MISS = bytes.fromhex(
    "0039303430303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932"
)
NUVEI_JSON_HIT = bytes.fromhex(
    "0000002c0100010030303030303032307b224163637074724175746873746e526571223a7b2276616c7565223a317d7d"
)
NUVEI_JSON_RESPONSE = bytes.fromhex(
    "0000002d0100010030303030303032307b224163637074724175746873746e5273706e223a7b2276616c7565223a327d7d"
)


def nuvei_frame(payload: bytes) -> bytes:
    body = bytes.fromhex("01000100") + b"00000020" + payload
    return len(body).to_bytes(4, "big") + body


def roundtrip(port: int, request: bytes, expected: bytes) -> dict:
    with socket.create_connection((REMOTE, port), timeout=3) as client:
        client.settimeout(5)
        client.sendall(request)
        response = bytearray()
        while len(response) < len(expected):
            chunk = client.recv(len(expected) - len(response))
            if not chunk:
                break
            response.extend(chunk)
    actual = bytes(response)
    if actual != expected:
        raise AssertionError(
            f"port {port}: expected {expected.hex()}, received {actual.hex()}"
        )
    return {
        "port": port,
        "request_hex": request.hex(),
        "expected_hex": expected.hex(),
        "actual_hex": actual.hex(),
        "result": "PASS",
    }


def invalid_frame(port: int, request: bytes) -> dict:
    with socket.create_connection((REMOTE, port), timeout=3) as client:
        client.settimeout(1.5)
        client.sendall(request)
        client.shutdown(socket.SHUT_WR)
        try:
            response = client.recv(4096)
        except socket.timeout:
            response = b""
    if response:
        raise AssertionError(f"port {port}: invalid frame returned {response.hex()}")
    return {
        "port": port,
        "request_hex": request.hex(),
        "actual_hex": "",
        "result": "PASS_FAIL_CLOSED",
    }


def main() -> None:
    rhai_hit = nuvei_frame((BASE / "resources/nuvei-rhai-request.json").read_bytes())
    rhai_response = nuvei_frame((BASE / "resources/nuvei-rhai-response.json").read_bytes())
    nuvei_miss = nuvei_frame(b'{"Other":{"value":1}}')
    results = {
        "hits": [
            roundtrip(8083, AU_REQUEST, AU_RESPONSE),
            roundtrip(8084, ISO_STANDARD_HIT, ISO_STANDARD_CHANGED),
            roundtrip(8085, ISO_DENO_HIT, ISO_DENO_CHANGED),
            roundtrip(8086, NUVEI_JSON_HIT, NUVEI_JSON_RESPONSE),
            roundtrip(8087, rhai_hit, rhai_response),
        ],
        "misses": [
            roundtrip(8084, ISO_STANDARD_MISS, ISO_STANDARD_MISS),
            roundtrip(8085, ISO_DENO_MISS, ISO_DENO_MISS),
            roundtrip(8086, nuvei_miss, nuvei_miss),
            roundtrip(8087, nuvei_miss, nuvei_miss),
        ],
        "invalid_frames": [
            invalid_frame(8084, bytes.fromhex("0003616263")),
            invalid_frame(8085, bytes.fromhex("0003616263")),
            invalid_frame(8086, bytes.fromhex("0000000d")),
            invalid_frame(8087, bytes.fromhex("0000000d")),
        ],
    }
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
