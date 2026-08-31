#!/usr/bin/env python3
"""Mounted-DMG release acceptance with an isolated macOS application profile."""

from __future__ import annotations

import argparse
import http.client
import json
import os
import plistlib
import queue
import signal
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

from e2e_socket_cases import ISO8583_SAMPLE_HEX

PROTOCOL_VERSION = "2026-07-28"
PACKAGE = {"id": "iso8583-ascii-standard", "version": "1.0.0"}


class AcceptanceError(RuntimeError):
    pass


def command(*arguments: str, capture: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        check=True,
        text=True,
        capture_output=capture,
    )


def require_free_port(port: int) -> None:
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
        except OSError as error:
            raise AcceptanceError(f"required port is unavailable: {port}") from error


def mcp_call(port: int, request_id: int, name: str, arguments: dict[str, Any]) -> Any:
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION,
                    "io.modelcontextprotocol/clientInfo": {
                        "name": "phase18-mounted-release",
                        "version": "1.0.0",
                    },
                    "io.modelcontextprotocol/clientCapabilities": {},
                },
                "name": name,
                "arguments": arguments,
            },
        },
        separators=(",", ":"),
    )
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    connection.request(
        "POST",
        "/mcp",
        body=body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "MCP-Protocol-Version": PROTOCOL_VERSION,
            "Mcp-Method": "tools/call",
            "Mcp-Name": name,
        },
    )
    response = connection.getresponse()
    payload = response.read()
    connection.close()
    if response.status != 200:
        raise AcceptanceError(f"MCP {name} returned HTTP {response.status}: {payload!r}")
    envelope = json.loads(payload)
    result = envelope.get("result", {})
    if result.get("isError"):
        raise AcceptanceError(f"MCP {name} failed: {result.get('structuredContent')}")
    return result.get("structuredContent")


def wait_for_mcp(port: int, process: subprocess.Popen[bytes], timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise AcceptanceError(f"mounted App exited during startup: {process.returncode}")
        try:
            mcp_call(port, 1, "workspace_list", {})
            return
        except (OSError, AcceptanceError, json.JSONDecodeError):
            time.sleep(0.1)
    raise AcceptanceError("mounted App MCP did not become ready")


def child_sidecars(process_id: int) -> list[int]:
    result = subprocess.run(
        ["pgrep", "-P", str(process_id), "-f", "intercept-proxy-package-sidecar"],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode not in (0, 1):
        raise AcceptanceError(f"pgrep failed: {result.stderr}")
    return [int(value) for value in result.stdout.split()]


def wait_for_sidecar(process_id: int, timeout: float) -> list[int]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        processes = child_sidecars(process_id)
        if processes:
            return processes
        time.sleep(0.1)
    raise AcceptanceError("bundled Boa sidecar was not started by the mounted App")


def candidate(args: argparse.Namespace) -> dict[str, Any]:
    common = {
        "enabled": True,
        "bind_address": "127.0.0.1",
        "connect_timeout_ms": 5_000,
        "read_timeout_ms": 10_000,
        "write_timeout_ms": 10_000,
    }
    return {
        "schema_version": 1,
        "target": {"mode": "new", "name": "Phase18 mounted release"},
        "workspace": {
            "listeners": [
                {
                    **common,
                    "alias": "phase18-http",
                    "name": "Phase18 HTTP",
                    "port": args.http_proxy_port,
                    "data_plane": {
                        "kind": "http",
                        "settings": {
                            "authentication": {"mode": "none"},
                            "mitm": {
                                "enabled": False,
                                "authority_allowlist": [],
                                "root_ca_selector": None,
                                "maximum_cached_leaf_certificates": 256,
                            },
                            "downstream_tls": {
                                "enabled": False,
                                "server_identity_alias": None,
                                "dynamic_sni_allowlist": [],
                                "client_authentication": {"mode": "disabled"},
                            },
                            "request_body_codec": "raw",
                            "response_body_codec": "raw",
                            "body_processing": {"mode": "plain"},
                            "fixed_server": {
                                "upstream_url": f"http://127.0.0.1:{args.http_upstream_port}",
                                "upstream_tls": {
                                    "verify_hostname": True,
                                    "server_trust_alias": None,
                                    "client_identity_alias": None,
                                },
                            },
                        },
                    },
                },
                {
                    **common,
                    "alias": "phase18-socket",
                    "name": "Phase18 Socket",
                    "port": args.socket_proxy_port,
                    "data_plane": {
                        "kind": "socket",
                        "settings": {
                            "topology": {
                                "mode": "relay",
                                "settings": {
                                    "upstream": {
                                        "host": "127.0.0.1",
                                        "port": args.socket_upstream_port,
                                    },
                                    "security": {"mode": "transparent"},
                                },
                            },
                            "maximum_connections": 8,
                            "runtime_limits": {
                                "read_chunk_bytes": 16_384,
                                "diagnostic_event_capacity": 256,
                                "diagnostic_memory_bytes": 1_048_576,
                            },
                            "processing": {
                                "mode": "scripted",
                                "settings": {"package": PACKAGE},
                            },
                        },
                    },
                },
            ],
            "rules": [],
            "android_network_profiles": [],
        },
        "materials": {"certificates": [], "secrets": []},
    }


def apply_candidate(port: int, args: argparse.Namespace) -> dict[str, Any]:
    created = mcp_call(
        port,
        10,
        "environment_candidate_create",
        {"candidate": candidate(args)},
    )
    if created.get("status") != "preview_ready":
        raise AcceptanceError(f"candidate preview failed: {created}")
    queued = mcp_call(
        port,
        11,
        "environment_candidate_apply",
        {
            "candidate_id": created["candidate_id"],
            "confirmation_token": created["confirmation_token"],
        },
    )
    if queued.get("status") != "apply_queued":
        raise AcceptanceError(f"candidate apply was not queued: {queued}")
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline:
        status = mcp_call(
            port,
            12,
            "environment_candidate_status",
            {"candidate_id": created["candidate_id"]},
        )
        if status.get("status") == "committed":
            return status
        if status.get("status") not in ("apply_queued", "apply_in_progress"):
            raise AcceptanceError(f"candidate reached non-committed terminal state: {status}")
        time.sleep(0.1)
    raise AcceptanceError("candidate apply did not reach committed state")


def http_byte_chain(proxy_port: int, upstream_port: int, timeout: float) -> dict[str, Any]:
    request_body = b"phase18-http-request-\x00\xff"
    response_body = b"phase18-http-response-\x01\xfe"
    received: queue.Queue[bytes | BaseException] = queue.Queue()
    ready = threading.Event()

    def server() -> None:
        try:
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", upstream_port))
                listener.listen(1)
                listener.settimeout(timeout)
                ready.set()
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(timeout)
                    raw = bytearray()
                    while b"\r\n\r\n" not in raw:
                        raw.extend(connection.recv(4096))
                    head, body = bytes(raw).split(b"\r\n\r\n", 1)
                    length = next(
                        int(line.split(b":", 1)[1])
                        for line in head.split(b"\r\n")
                        if line.lower().startswith(b"content-length:")
                    )
                    while len(body) < length:
                        body += connection.recv(length - len(body))
                    received.put(body)
                    connection.sendall(
                        b"HTTP/1.1 200 OK\r\nContent-Length: "
                        + str(len(response_body)).encode()
                        + b"\r\nConnection: close\r\n\r\n"
                        + response_body
                    )
        except BaseException as error:
            received.put(error)
            ready.set()

    thread = threading.Thread(target=server, daemon=True)
    thread.start()
    if not ready.wait(timeout):
        raise AcceptanceError("HTTP upstream did not become ready")
    connection = http.client.HTTPConnection("127.0.0.1", proxy_port, timeout=timeout)
    connection.request("POST", "/phase18", body=request_body, headers={"Content-Type": "application/octet-stream"})
    response = connection.getresponse()
    actual_response = response.read()
    connection.close()
    thread.join(timeout)
    actual_request = received.get(timeout=timeout)
    if isinstance(actual_request, BaseException):
        raise AcceptanceError(f"HTTP upstream failed: {actual_request}") from actual_request
    if response.status != 200 or actual_request != request_body or actual_response != response_body:
        raise AcceptanceError("HTTP mounted-App byte chain changed request or response bytes")
    return {"request_bytes": len(request_body), "response_bytes": len(response_body)}


def receive_frame(connection: socket.socket) -> bytes:
    prefix = connection.recv(2)
    if len(prefix) != 2:
        raise AcceptanceError("Socket frame prefix was incomplete")
    remaining = int.from_bytes(prefix, "big")
    payload = bytearray()
    while len(payload) < remaining:
        chunk = connection.recv(remaining - len(payload))
        if not chunk:
            raise AcceptanceError("Socket frame ended early")
        payload.extend(chunk)
    return prefix + payload


def socket_byte_chain(proxy_port: int, upstream_port: int, timeout: float) -> dict[str, Any]:
    request = bytes.fromhex(ISO8583_SAMPLE_HEX)
    response = request[:2] + b"0210" + request[6:]
    received: queue.Queue[bytes | BaseException] = queue.Queue()
    ready = threading.Event()

    def server() -> None:
        try:
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", upstream_port))
                listener.listen(1)
                listener.settimeout(timeout)
                ready.set()
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(timeout)
                    received.put(receive_frame(connection))
                    connection.sendall(response)
        except BaseException as error:
            received.put(error)
            ready.set()

    thread = threading.Thread(target=server, daemon=True)
    thread.start()
    if not ready.wait(timeout):
        raise AcceptanceError("Socket upstream did not become ready")
    with socket.create_connection(("127.0.0.1", proxy_port), timeout) as client:
        client.settimeout(timeout)
        client.sendall(request)
        actual_response = receive_frame(client)
    thread.join(timeout)
    actual_request = received.get(timeout=timeout)
    if isinstance(actual_request, BaseException):
        raise AcceptanceError(f"Socket upstream failed: {actual_request}") from actual_request
    if actual_request != request or actual_response != response:
        raise AcceptanceError("official ZIP Socket byte chain changed an unchanged Document")
    return {"request_bytes": len(request), "response_bytes": len(response)}


def launch(executable: Path, profile: Path) -> subprocess.Popen[bytes]:
    environment = os.environ.copy()
    environment["HOME"] = str(profile)
    environment["CFFIXED_USER_HOME"] = str(profile)
    return subprocess.Popen([str(executable)], env=environment, start_new_session=True)


def quit_app(process: subprocess.Popen[bytes], sidecars: list[int], timeout: float) -> None:
    command("osascript", "-e", 'tell application id "com.interceptproxy.desktop" to quit')
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise AcceptanceError("mounted App did not complete graceful quit") from error
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if all(not process_alive(process_id) for process_id in sidecars):
            return
        time.sleep(0.1)
    raise AcceptanceError(f"orphaned bundled sidecar process IDs: {sidecars}")


def process_alive(process_id: int) -> bool:
    try:
        os.kill(process_id, 0)
        return True
    except ProcessLookupError:
        return False


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dmg", type=Path, required=True)
    parser.add_argument("--mcp-port", type=int, required=True)
    parser.add_argument("--http-proxy-port", type=int, required=True)
    parser.add_argument("--http-upstream-port", type=int, required=True)
    parser.add_argument("--socket-proxy-port", type=int, required=True)
    parser.add_argument("--socket-upstream-port", type=int, required=True)
    parser.add_argument("--timeout", type=float, default=20.0)
    args = parser.parse_args()
    if args.mcp_port != 17653:
        raise AcceptanceError("production MCP port is fixed by the current contract at 17653")
    ports = [args.mcp_port, args.http_proxy_port, args.http_upstream_port, args.socket_proxy_port, args.socket_upstream_port]
    if len(set(ports)) != len(ports):
        raise AcceptanceError("all explicit E2E ports must be distinct")
    for port in ports:
        require_free_port(port)

    mounted: Path | None = None
    process: subprocess.Popen[bytes] | None = None
    with tempfile.TemporaryDirectory(prefix="phase18-profile-") as profile_name:
        profile = Path(profile_name)
        try:
            attached = command(
                "hdiutil", "attach", "-plist", "-readonly", "-nobrowse", str(args.dmg.resolve()), capture=True
            )
            document = plistlib.loads(attached.stdout.encode())
            mounted = next(
                Path(entity["mount-point"])
                for entity in document["system-entities"]
                if "mount-point" in entity
            )
            app = next(mounted.glob("*.app"))
            executable = app / "Contents/MacOS/intercept-proxy"
            results: dict[str, Any] = {"mounted_app": str(app), "profile": str(profile)}
            first_workspace: dict[str, Any] | None = None
            for start in (1, 2):
                process = launch(executable, profile)
                wait_for_mcp(args.mcp_port, process, args.timeout)
                sidecars = wait_for_sidecar(process.pid, args.timeout)
                if start == 1:
                    apply_candidate(args.mcp_port, args)
                workspaces = mcp_call(args.mcp_port, 20 + start, "workspace_list", {})
                selected = next(item for item in workspaces if item["name"] == "Phase18 mounted release")
                if start == 1:
                    first_workspace = selected
                elif selected != first_workspace:
                    raise AcceptanceError("Workspace identity/revision changed across mounted-App restart")
                results[f"start_{start}"] = {
                    "workspace": selected,
                    "http": http_byte_chain(args.http_proxy_port, args.http_upstream_port, args.timeout),
                    "socket": socket_byte_chain(args.socket_proxy_port, args.socket_upstream_port, args.timeout),
                    "sidecar_pids": sidecars,
                }
                quit_app(process, sidecars, args.timeout)
                process = None
            database = profile / "Library/Application Support/com.interceptproxy.desktop/intercept-proxy.sqlite3"
            if not database.is_file():
                raise AcceptanceError(f"isolated Schema100 database missing: {database}")
            results["database"] = str(database)
            print(json.dumps(results, indent=2))
        finally:
            if process is not None and process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=args.timeout)
            if mounted is not None:
                command("hdiutil", "detach", str(mounted))


if __name__ == "__main__":
    main()
