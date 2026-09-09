#!/usr/bin/env python3
"""Run the local macOS HTTP and Socket wire regression matrix.

The application configuration and listener start are deliberately external to
this script.  It starts both upstream servers, publishes a READY artifact, and
waits for outputs/wire/start-clients before sending any proxy traffic.
"""

from __future__ import annotations

import argparse
import json
import queue
import socket
import sys
import threading
import time
import traceback
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[7]
sys.path.insert(0, str(REPO / "scripts"))

from e2e_macos_mounted_release import mcp_call  # noqa: E402

EVIDENCE = Path(__file__).resolve().parents[1]
INPUTS = EVIDENCE / "inputs" / "wire"
OUTPUTS = EVIDENCE / "outputs" / "wire"
START_MARKER = OUTPUTS / "start-clients"
OLD_SOCKET_INPUTS = (
    REPO
    / "docs/testing/evidence/2026-09-01/TASK-20260901-001"
    / "release-app-replay/inputs/socket"
)
HTTP_PROXY_PORT = 28080
HTTP_UPSTREAM_PORT = 28081
SOCKET_PROXY_PORT = 28082
SOCKET_UPSTREAM_PORT = 28083


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def write_wire(path: Path, value: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.with_suffix(".bin").write_bytes(value)
    path.with_suffix(".hex").write_text(value.hex() + "\n")


def recv_until_eof(connection: socket.socket, timeout: float = 8) -> tuple[bytes, list[bytes], str]:
    connection.settimeout(timeout)
    chunks: list[bytes] = []
    while True:
        try:
            chunk = connection.recv(4096)
        except TimeoutError:
            return b"".join(chunks), chunks, "timeout"
        except OSError as error:
            return b"".join(chunks), chunks, f"error:{type(error).__name__}:{error}"
        if not chunk:
            return b"".join(chunks), chunks, "eof"
        chunks.append(chunk)


def recv_http_request(connection: socket.socket) -> tuple[bytes, list[bytes]] | None:
    connection.settimeout(2)
    chunks: list[bytes] = []
    raw = bytearray()
    try:
        while b"\r\n\r\n" not in raw:
            chunk = connection.recv(4096)
            if not chunk:
                return None
            chunks.append(chunk)
            raw.extend(chunk)
        head, body = bytes(raw).split(b"\r\n\r\n", 1)
        length = 0
        for line in head.split(b"\r\n")[1:]:
            if line.lower().startswith(b"content-length:"):
                length = int(line.split(b":", 1)[1].strip())
        while len(body) < length:
            chunk = connection.recv(length - len(body))
            if not chunk:
                return None
            chunks.append(chunk)
            body += chunk
        return head + b"\r\n\r\n" + body[:length], chunks
    except TimeoutError:
        return None


def parse_http(raw: bytes) -> tuple[str, str, dict[str, list[str]], bytes]:
    head, body = raw.split(b"\r\n\r\n", 1)
    lines = head.split(b"\r\n")
    method, target, _ = lines[0].decode("latin-1").split(" ", 2)
    headers: dict[str, list[str]] = {}
    for line in lines[1:]:
        name, value = line.split(b":", 1)
        headers.setdefault(name.decode("latin-1").lower(), []).append(value.decode("latin-1").strip())
    return method, target, headers, body


class Upstreams:
    def __init__(self) -> None:
        self.stop = threading.Event()
        self.ready_http = threading.Event()
        self.ready_socket = threading.Event()
        self.errors: queue.Queue[BaseException] = queue.Queue()
        self.http_records: list[dict[str, Any]] = []
        self.socket_records: list[dict[str, Any]] = []
        self.threads = [
            threading.Thread(target=self._guard, args=(self._http,), name="wire-http-upstream"),
            threading.Thread(target=self._guard, args=(self._socket,), name="wire-socket-upstream"),
        ]

    def _guard(self, target: Any) -> None:
        try:
            target()
        except BaseException as error:
            self.errors.put(error)

    def start(self) -> None:
        for thread in self.threads:
            thread.start()
        if not self.ready_http.wait(5) or not self.ready_socket.wait(5):
            raise RuntimeError("upstream servers did not become ready")
        if not self.errors.empty():
            raise self.errors.get()

    def close(self) -> None:
        self.stop.set()
        for port in (HTTP_UPSTREAM_PORT, SOCKET_UPSTREAM_PORT):
            try:
                socket.create_connection(("127.0.0.1", port), timeout=1).close()
            except OSError:
                pass
        for thread in self.threads:
            thread.join(5)
        live = [thread.name for thread in self.threads if thread.is_alive()]
        if live:
            raise RuntimeError(f"upstream threads failed to stop: {live}")
        if not self.errors.empty():
            raise self.errors.get()

    def _http(self) -> None:
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", HTTP_UPSTREAM_PORT))
            listener.listen(16)
            listener.settimeout(0.5)
            self.ready_http.set()
            while not self.stop.is_set():
                try:
                    connection, address = listener.accept()
                except TimeoutError:
                    continue
                with connection:
                    parsed = recv_http_request(connection)
                    if parsed is None:
                        continue
                    raw, chunks = parsed
                    method, target, headers, body = parse_http(raw)
                    case = headers.get("x-wire-case", [None])[0]
                    if case is None:
                        raise AssertionError("HTTP upstream request lacks X-Wire-Case")
                    record = {
                        "case": case,
                        "remote": f"{address[0]}:{address[1]}",
                        "method": method,
                        "target": target,
                        "headers": headers,
                        "body_hex": body.hex(),
                        "body_text": body.decode("utf-8", errors="replace"),
                    }
                    self.http_records.append(record)
                    write_wire(OUTPUTS / "http" / case / "server-request", raw)
                    write_json(OUTPUTS / "http" / case / "server-request-chunks.json", [x.hex() for x in chunks])
                    write_json(OUTPUTS / "http" / case / "server-record.json", record)
                    payload = json.dumps(record, ensure_ascii=False, separators=(",", ":")).encode()
                    response = (
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
                        + str(len(payload)).encode()
                        + b"\r\nConnection: close\r\n\r\n"
                        + payload
                    )
                    write_wire(OUTPUTS / "http" / case / "server-response", response)
                    connection.sendall(response)

    def _socket(self) -> None:
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", SOCKET_UPSTREAM_PORT))
            listener.listen(16)
            listener.settimeout(0.5)
            self.ready_socket.set()
            while not self.stop.is_set():
                try:
                    connection, address = listener.accept()
                except TimeoutError:
                    continue
                with connection:
                    connection.settimeout(2)
                    chunks: list[bytes] = []
                    raw = bytearray()
                    try:
                        while len(raw) < 2:
                            chunk = connection.recv(2 - len(raw))
                            if not chunk:
                                break
                            chunks.append(chunk)
                            raw.extend(chunk)
                        if len(raw) < 2:
                            continue
                        length = int.from_bytes(raw[:2], "big")
                        while len(raw) < length + 2:
                            chunk = connection.recv(length + 2 - len(raw))
                            if not chunk:
                                break
                            chunks.append(chunk)
                            raw.extend(chunk)
                    except TimeoutError:
                        continue
                    if len(raw) != length + 2:
                        continue
                    frame = bytes(raw)
                    case = "match" if frame[2:6] == b"0100" else "miss" if frame[2:6] == b"0400" else "unexpected"
                    record = {"case": case, "remote": f"{address[0]}:{address[1]}", "frame_hex": frame.hex()}
                    self.socket_records.append(record)
                    write_wire(OUTPUTS / "socket" / case / "server-frame", frame)
                    write_json(OUTPUTS / "socket" / case / "server-chunks.json", [x.hex() for x in chunks])
                    write_json(OUTPUTS / "socket" / case / "server-record.json", record)
                    connection.sendall(frame)


def http_request(name: str, method: str, target: str, document: Any, headers: dict[str, str], invalid: bool = False) -> dict[str, Any]:
    body = b"{" if invalid else json.dumps(document).encode()
    raw = (
        f"{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{HTTP_PROXY_PORT}\r\n"
        f"Content-Type: application/json\r\nContent-Length: {len(body)}\r\n"
        f"Connection: close\r\nX-Wire-Case: {name}\r\n"
        + "".join(f"{key}: {value}\r\n" for key, value in headers.items())
        + "\r\n"
    ).encode() + body
    write_wire(INPUTS / "http" / name / "client-request", raw)
    with socket.create_connection(("127.0.0.1", HTTP_PROXY_PORT), timeout=5) as connection:
        local_port = connection.getsockname()[1]
        connection.sendall(raw)
        response, chunks, end = recv_until_eof(connection)
    write_wire(OUTPUTS / "http" / name / "client-response", response)
    write_json(OUTPUTS / "http" / name / "client-response-chunks.json", [x.hex() for x in chunks])
    result = {"case": name, "local_port": local_port, "end": end, "response_hex": response.hex()}
    write_json(OUTPUTS / "http" / name / "client-result.json", result)
    return result


def socket_request(name: str, frame: bytes, invalid: bool = False) -> dict[str, Any]:
    write_wire(INPUTS / "socket" / name / "client-frame", frame)
    with socket.create_connection(("127.0.0.1", SOCKET_PROXY_PORT), timeout=5) as connection:
        local_port = connection.getsockname()[1]
        connection.sendall(frame)
        connection.shutdown(socket.SHUT_WR)
        response, chunks, end = recv_until_eof(connection)
    write_wire(OUTPUTS / "socket" / name / "client-response", response)
    write_json(OUTPUTS / "socket" / name / "client-response-chunks.json", [x.hex() for x in chunks])
    result = {"case": name, "local_port": local_port, "end": end, "response_hex": response.hex(), "invalid": invalid}
    write_json(OUTPUTS / "socket" / name / "client-result.json", result)
    return result


def parse_http_response(raw: bytes) -> tuple[int, bytes]:
    head, body = raw.split(b"\r\n\r\n", 1)
    status = int(head.split(b"\r\n", 1)[0].split()[1])
    return status, body


def wait_for_marker(seconds: int) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if START_MARKER.is_file():
            return
        time.sleep(0.2)
    raise TimeoutError(f"start marker not created within {seconds}s: {START_MARKER}")


def find_listener(workspace: dict[str, Any], port: int) -> dict[str, Any]:
    return next(item for item in workspace["listeners"] if item["port"] == port)


def observations_by_port(value: dict[str, Any], ports: set[int]) -> dict[int, dict[str, Any]]:
    result: dict[int, dict[str, Any]] = {}
    for row in value["rows"]:
        try:
            port = int(row["peer_address"].rsplit(":", 1)[1])
        except (KeyError, ValueError):
            continue
        if port in ports:
            result[port] = row
    return result


def assert_decode_failure(row: dict[str, Any], case: str) -> None:
    failed = [event for event in row["events"] if event.get("event") == "failed"]
    if len(failed) != 1 or failed[0].get("stage") != "decode" or not failed[0].get("error"):
        raise AssertionError(f"{case}: missing MCP decode failure: {row}")
    closed = [event for event in row["events"] if event.get("event") == "closed"]
    if len(closed) != 1 or closed[0].get("outcome") != "failed":
        raise AssertionError(f"{case}: missing MCP failed close: {row}")


def run(mcp_port: int, marker_wait: int) -> dict[str, Any]:
    OUTPUTS.mkdir(parents=True, exist_ok=True)
    INPUTS.mkdir(parents=True, exist_ok=True)
    if START_MARKER.exists():
        raise RuntimeError(f"refusing stale start marker: {START_MARKER}")
    upstreams = Upstreams()
    upstreams.start()
    write_json(OUTPUTS / "servers-ready.json", {"http": HTTP_UPSTREAM_PORT, "socket": SOCKET_UPSTREAM_PORT})
    print(json.dumps({"state": "READY", "start_marker": str(START_MARKER)}), flush=True)
    try:
        wait_for_marker(marker_wait)
        workspaces = mcp_call(mcp_port, 1, "workspace_list", {})
        selected = next(item for item in workspaces if item["selected"])
        workspace = mcp_call(mcp_port, 2, "workspace_get", {"workspace_id": selected["id"]})
        http_listener = find_listener(workspace, HTTP_PROXY_PORT)
        socket_listener = find_listener(workspace, SOCKET_PROXY_PORT)
        rules_before = mcp_call(mcp_port, 3, "workspace_rule_list", {"workspace_id": selected["id"]})
        write_json(OUTPUTS / "mcp" / "workspace-list.json", workspaces)
        write_json(OUTPUTS / "mcp" / "workspace.json", workspace)
        write_json(OUTPUTS / "mcp" / "rules-before.json", rules_before)

        cases = [
            ("method", "POST", "/other", {"customer": {"age": 17, "name": "original"}}, {}),
            ("header", "GET", "/other", {"customer": {"age": 17, "name": "original"}}, {"X-Probe": "alpha"}),
            ("path", "GET", "/orders/123?mode=test", {"customer": {"age": 17, "name": "original"}}, {}),
            ("body", "GET", "/other", {"customer": {"age": 18, "name": "original"}}, {}),
            ("all", "POST", "/orders/123?mode=test", {"customer": {"age": 18, "name": "original"}}, {"X-Probe": "alpha"}),
            ("miss", "GET", "/other", {"customer": {"age": 17, "name": "original"}}, {}),
        ]
        http_results = [http_request(*case) for case in cases]
        invalid_http = http_request("invalid", "POST", "/other", None, {}, invalid=True)

        source_frames = {}
        for name, filename in (("match", "socket-match-0200.hex"), ("miss", "socket-miss-0400.hex"), ("invalid", "socket-invalid.hex")):
            source_frames[name] = bytes.fromhex((OLD_SOCKET_INPUTS / filename).read_text().strip())
        expected_match = source_frames["match"][:2] + b"0100" + source_frames["match"][6:]
        socket_results = [
            socket_request("match", source_frames["match"]),
            socket_request("miss", source_frames["miss"]),
            socket_request("invalid", source_frames["invalid"], invalid=True),
        ]

        time.sleep(0.5)
        rules_after = mcp_call(mcp_port, 10, "workspace_rule_list", {"workspace_id": selected["id"]})
        captures = mcp_call(mcp_port, 11, "http_capture_query", {"page": 1, "page_size": 100})
        wanted_http = {item["local_port"] for item in http_results + [invalid_http]}
        wanted_socket = {item["local_port"] for item in socket_results}
        deadline = time.monotonic() + 10
        while True:
            http_observations = mcp_call(mcp_port, 12, "exchange_observation_query", {"workspace_id": selected["id"], "listener_id": http_listener["id"], "page": {"page": 1, "page_size": 100}})
            socket_observations = mcp_call(mcp_port, 13, "exchange_observation_query", {"workspace_id": selected["id"], "listener_id": socket_listener["id"], "page": {"page": 1, "page_size": 100}})
            if wanted_http <= observations_by_port(http_observations, wanted_http).keys() and wanted_socket <= observations_by_port(socket_observations, wanted_socket).keys():
                break
            if time.monotonic() >= deadline:
                raise AssertionError("MCP observations did not contain all ten client connections")
            time.sleep(0.2)
        write_json(OUTPUTS / "mcp" / "rules-after.json", rules_after)
        write_json(OUTPUTS / "mcp" / "http-captures.json", captures)
        write_json(OUTPUTS / "mcp" / "http-observations.json", http_observations)
        write_json(OUTPUTS / "mcp" / "socket-observations.json", socket_observations)

        if len(upstreams.http_records) != 6:
            raise AssertionError(f"HTTP upstream expected 6 records, got {len(upstreams.http_records)}")
        if [record["case"] for record in upstreams.socket_records] != ["match", "miss"]:
            raise AssertionError(f"Socket upstream expected exact match/miss frames: {upstreams.socket_records}")
        server_http = {record["case"]: record for record in upstreams.http_records}
        for result in http_results:
            if result["end"] != "eof":
                raise AssertionError(f"HTTP client did not complete cleanly: {result}")
            status, body = parse_http_response(bytes.fromhex(result["response_hex"]))
            if status != 200 or json.loads(body) != server_http[result["case"]]:
                raise AssertionError(f"{result['case']}: HTTP client response differs from upstream response")
            headers = server_http[result["case"]]["headers"]
            expected = {
                "x-method-hit": result["case"] in ("method", "all"),
                "x-header-hit": result["case"] in ("header", "all"),
                "x-path-hit": result["case"] in ("path", "all"),
            }
            for header, present in expected.items():
                if headers.get(header, []) != (["yes"] if present else []):
                    raise AssertionError(f"{result['case']}: {header} assertion failed")
            body_document = json.loads(bytes.fromhex(server_http[result["case"]]["body_hex"]))
            expected_name = "matched" if result["case"] in ("body", "all") else "original"
            if body_document["customer"]["name"] != expected_name:
                raise AssertionError(f"{result['case']}: document rule assertion failed")
        if invalid_http["end"] == "timeout" or invalid_http["response_hex"]:
            raise AssertionError(f"invalid HTTP must end by EOF/error without upstream response: {invalid_http}")
        if socket_results[0]["response_hex"] != expected_match.hex():
            raise AssertionError("Socket match response is not exact rewritten upstream echo")
        if socket_results[1]["response_hex"] != source_frames["miss"].hex():
            raise AssertionError("Socket miss response is not byte-identical upstream echo")
        if socket_results[2]["end"] == "timeout" or socket_results[2]["response_hex"]:
            raise AssertionError(f"invalid Socket must end by EOF/error without response: {socket_results[2]}")
        if upstreams.socket_records[0]["frame_hex"] != expected_match.hex() or upstreams.socket_records[1]["frame_hex"] != source_frames["miss"].hex():
            raise AssertionError("Socket upstream did not receive the exact two expected frames")

        http_by_port = observations_by_port(http_observations, wanted_http)
        socket_by_port = observations_by_port(socket_observations, wanted_socket)
        assert_decode_failure(http_by_port[invalid_http["local_port"]], "invalid HTTP")
        assert_decode_failure(socket_by_port[socket_results[2]["local_port"]], "invalid Socket")
        invalid_http_row = http_by_port[invalid_http["local_port"]]
        if not any(event.get("error", "").startswith("Invalid Document JSON:") for event in invalid_http_row["events"] if event.get("event") == "failed"):
            raise AssertionError("invalid HTTP did not produce the current JSON decode error message")
        for invalid_row in (invalid_http_row, socket_by_port[socket_results[2]["local_port"]]):
            if any(event["event"] == "sent" for event in invalid_row["events"]):
                raise AssertionError("invalid input was sent by proxy")
        for result in http_results + socket_results[:2]:
            by_port = http_by_port if result in http_results else socket_by_port
            events = by_port[result["local_port"]]["events"]
            if not any(event.get("event") == "closed" and event.get("outcome") == "completed" for event in events):
                raise AssertionError("successful wire response lacks completed observation")
        before_hits = {rule["name"]: rule["lifecycle"]["hit_count"] for rule in rules_before}
        after_hits = {rule["name"]: rule["lifecycle"]["hit_count"] for rule in rules_after}
        expected_deltas = {"Method POST": 2, "Header X-Probe alpha": 2, "Path and query wildcard": 2, "Body age 18": 2, "MTI 0200 to 0100": 1}
        deltas = {name: after_hits[name] - before_hits[name] for name in expected_deltas}
        if deltas != expected_deltas:
            raise AssertionError(f"rule hit deltas mismatch: expected {expected_deltas}, got {deltas}")
        result = {"result": "PASS", "http_cases": 7, "http_upstream_records": 6, "socket_cases": 3, "socket_upstream_frames": 2, "rule_hit_deltas": deltas, "invalid_decode_failures": 2}
        write_json(OUTPUTS / "summary.json", result)
        return result
    finally:
        upstreams.close()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mcp-port", type=int, default=17653)
    parser.add_argument("--marker-wait-seconds", type=int, default=600)
    args = parser.parse_args()
    try:
        result = run(args.mcp_port, args.marker_wait_seconds)
    except BaseException as error:
        result = {"result": "FAIL", "error_type": type(error).__name__, "error": str(error), "traceback": traceback.format_exc()}
        write_json(OUTPUTS / "summary.json", result)
        print(json.dumps(result, ensure_ascii=False), file=sys.stderr)
        raise SystemExit(1)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
