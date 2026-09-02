#!/usr/bin/env python3
"""Replay current App HTTP and Socket data planes against local mock servers."""

from __future__ import annotations

import http.client
import json
import socket
import socketserver
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any


MCP_HOST = "127.0.0.1"
MCP_PORT = 17653
MCP_PROTOCOL_VERSION = "2026-07-28"
CASE_MARKER = "running-app-replay-20260828"


class ReplayFailure(RuntimeError):
    pass


class MockHttpHandler(BaseHTTPRequestHandler):
    server_version = "InterceptProxyReplay/1.0"

    def do_POST(self) -> None:  # noqa: N802 - stdlib callback name
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        record = {
            "method": "POST",
            "path": self.path,
            "marker": self.headers.get("X-Replay-Case"),
            "body_utf8": body.decode("utf-8"),
        }
        self.server.records.append(record)  # type: ignore[attr-defined]
        response = json.dumps(
            {"server": "mock-http", "received": record},
            ensure_ascii=False,
            separators=(",", ":"),
        ).encode()
        self.send_response(201)
        self.send_header("Content-Type", "application/json")
        self.send_header("X-Mock-Server", CASE_MARKER)
        self.send_header("Content-Length", str(len(response)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(response)

    def log_message(self, _format: str, *_args: object) -> None:
        return


class ThreadedTcpServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = False
    daemon_threads = True


class EchoHandler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        payload = self.request.recv(65536)
        self.server.records.append(payload)  # type: ignore[attr-defined]
        self.request.sendall(b"ECHO:" + payload)


def start_mock_servers() -> tuple[ThreadingHTTPServer, ThreadedTcpServer, list[threading.Thread]]:
    http_server = ThreadingHTTPServer(("127.0.0.1", 0), MockHttpHandler)
    http_server.records = []  # type: ignore[attr-defined]
    tcp_server = ThreadedTcpServer(("127.0.0.1", 0), EchoHandler)
    tcp_server.records = []  # type: ignore[attr-defined]
    threads = [
        threading.Thread(target=http_server.serve_forever, daemon=True),
        threading.Thread(target=tcp_server.serve_forever, daemon=True),
    ]
    for thread in threads:
        thread.start()
    return http_server, tcp_server, threads


def available_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as reserved:
        reserved.bind(("127.0.0.1", 0))
        return int(reserved.getsockname()[1])


def request_meta() -> dict[str, Any]:
    return {
        "io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientInfo": {
            "name": "running-app-archive-replay",
            "version": "1.0.0",
        },
        "io.modelcontextprotocol/clientCapabilities": {},
    }


def mcp_tool(name: str, arguments: dict[str, Any], request_id: int) -> Any:
    envelope = {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {
            "_meta": request_meta(),
            "name": name,
            "arguments": arguments,
        },
    }
    body = json.dumps(envelope, separators=(",", ":"))
    headers = {
        "Host": MCP_HOST,
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
        "MCP-Protocol-Version": MCP_PROTOCOL_VERSION,
        "Mcp-Method": "tools/call",
        "Mcp-Name": name,
        "Connection": "close",
    }
    connection = http.client.HTTPConnection(MCP_HOST, MCP_PORT, timeout=35)
    connection.request("POST", "/mcp", body=body, headers=headers)
    response = connection.getresponse()
    raw = response.read()
    connection.close()
    if response.status != 200:
        raise ReplayFailure(f"{name} HTTP {response.status}: {raw[:500]!r}")
    envelope = json.loads(raw)
    if "error" in envelope:
        raise ReplayFailure(f"{name} JSON-RPC error: {envelope['error']}")
    result = envelope["result"]
    if result.get("isError"):
        failure = result.get("structuredContent") or result.get("content")
        raise ReplayFailure(f"{name} tool error: {failure}")
    structured = result.get("structuredContent")
    if structured is not None:
        return structured
    return json.loads(result["content"][0]["text"])


def await_candidate(candidate_id: str, request_id: int) -> dict[str, Any]:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        status = mcp_tool(
            "environment_candidate_status",
            {"candidate_id": candidate_id},
            request_id,
        )
        if status["status"] not in {"apply_queued", "apply_in_progress"}:
            return status
        time.sleep(0.05)
    raise ReplayFailure(f"candidate {candidate_id} did not reach terminal state")


def apply_candidate(candidate: dict[str, Any], request_id: int) -> dict[str, Any]:
    created = mcp_tool(
        "environment_candidate_create", {"candidate": candidate}, request_id
    )
    if created.get("status") != "preview_ready":
        raise ReplayFailure(f"candidate preview failed: {created}")
    candidate_id = created["candidate_id"]
    queued = mcp_tool(
        "environment_candidate_apply",
        {
            "candidate_id": candidate_id,
            "confirmation_token": created["confirmation_token"],
        },
        request_id + 1,
    )
    if queued.get("status") != "apply_queued":
        raise ReplayFailure(f"candidate was not queued: {queued}")
    terminal = await_candidate(candidate_id, request_id + 2)
    if terminal.get("status") != "committed":
        raise ReplayFailure(f"candidate terminal was not committed: {terminal}")
    return {
        "candidate_id_present": bool(candidate_id),
        "preview_status": created["status"],
        "queue_status": queued["status"],
        "terminal_status": terminal["status"],
        "terminal_result": terminal.get("terminal_result", {}).get("result"),
    }


def http_listener(port: int, upstream_port: int) -> dict[str, Any]:
    return {
        "alias": "replay-http",
        "name": "Runtime replay HTTP",
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": port,
        "connect_timeout_ms": 5000,
        "read_timeout_ms": 10000,
        "write_timeout_ms": 10000,
        "data_plane": {
            "kind": "http",
            "settings": {
                "authentication": {"mode": "none"},
                "mitm": {
                    "enabled": False,
                    "authority_allowlist": [],
                    "root_ca_selector": None,
                    "maximum_cached_leaf_certificates": 64,
                },
                "downstream_tls": {
                    "enabled": False,
                    "server_identity_alias": None,
                    "dynamic_sni_allowlist": [],
                    "client_authentication": {"mode": "disabled"},
                },
                "request_body_codec": "auto",
                "response_body_codec": "utf8",
                "body_processing": {"mode": "plain"},
                "fixed_server": {
                    "upstream_url": f"http://127.0.0.1:{upstream_port}",
                    "upstream_tls": {
                        "verify_hostname": False,
                        "server_trust_alias": None,
                        "client_identity_alias": None,
                    },
                },
            },
        },
    }


def socket_listener(port: int, upstream_port: int) -> dict[str, Any]:
    return {
        "alias": "replay-socket",
        "name": "Runtime replay Socket",
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": port,
        "connect_timeout_ms": 5000,
        "read_timeout_ms": 10000,
        "write_timeout_ms": 10000,
        "data_plane": {
            "kind": "socket",
            "settings": {
                "topology": {
                    "mode": "relay",
                    "settings": {
                        "upstream": {"host": "127.0.0.1", "port": upstream_port},
                        "security": {"mode": "transparent"},
                    },
                },
                "maximum_connections": 16,
                "runtime_limits": {
                    "read_chunk_bytes": 4096,
                    "diagnostic_event_capacity": 256,
                    "diagnostic_memory_bytes": 1048576,
                },
                "processing": {"mode": "direct"},
            },
        },
    }


def candidate(
    workspace_id: str,
    revision: int,
    listeners: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "target": {
            "mode": "existing",
            "workspace_id": workspace_id,
            "expected_revision": revision,
        },
        "workspace": {
            "listeners": listeners,
            "http_rules": [],
            "protocol_rules": [],
            "android_network_profiles": [],
        },
        "materials": {"certificates": [], "secrets": []},
    }


def wait_for_listeners(
    expected: int, timeout_seconds: int = 10
) -> list[dict[str, Any]]:
    deadline = time.monotonic() + timeout_seconds
    last: list[dict[str, Any]] = []
    while time.monotonic() < deadline:
        last = mcp_tool("entry_status_list", {}, 40)
        if len(last) == expected:
            return last
        time.sleep(0.05)
    raise ReplayFailure(f"expected {expected} listener statuses, got {last}")


def run_http(proxy_port: int) -> dict[str, Any]:
    body = json.dumps({"case": CASE_MARKER, "amount": 1234}, separators=(",", ":"))
    connection = http.client.HTTPConnection("127.0.0.1", proxy_port, timeout=10)
    connection.request(
        "POST",
        f"/replay/http?case={CASE_MARKER}",
        body=body,
        headers={
            "Content-Type": "application/json",
            "X-Replay-Case": CASE_MARKER,
            "Connection": "close",
        },
    )
    response = connection.getresponse()
    response_body = response.read()
    result = {
        "status": response.status,
        "mock_header": response.getheader("X-Mock-Server"),
        "body": json.loads(response_body),
    }
    connection.close()
    return result


def run_socket(proxy_port: int) -> dict[str, Any]:
    payload = b"\x00\x15" + CASE_MARKER.encode()
    with socket.create_connection(("127.0.0.1", proxy_port), timeout=10) as client:
        client.sendall(payload)
        received = client.recv(65536)
    return {
        "sent_hex": payload.hex(),
        "received_hex": received.hex(),
        "expected_hex": (b"ECHO:" + payload).hex(),
    }


def main() -> None:
    http_server, tcp_server, _threads = start_mock_servers()
    original = mcp_tool("workspace_list", {}, 1)
    if len(original) != 1 or not original[0].get("selected"):
        raise ReplayFailure(f"expected one selected Workspace: {original}")
    workspace_id = original[0]["id"]
    original_detail = mcp_tool("workspace_get", {"workspace_id": workspace_id}, 2)
    if any(
        original_detail.get(field)
        for field in ("listeners", "rules", "protocol_rules", "android_network_profiles")
    ):
        raise ReplayFailure("current Workspace is not empty; refusing temporary replacement")

    http_proxy_port = available_port()
    socket_proxy_port = available_port()
    http_upstream_port = int(http_server.server_address[1])
    socket_upstream_port = int(tcp_server.server_address[1])
    test_applied = False
    restore_result: dict[str, Any] | None = None
    result: dict[str, Any] = {
        "case": "RUNNING-APP-REPLAY-001",
        "mcp_protocol_version": MCP_PROTOCOL_VERSION,
        "workspace_before": {
            "id_present": bool(workspace_id),
            "name": original_detail["name"],
            "revision": original_detail["revision"],
            "listener_count": len(original_detail["listeners"]),
            "rule_count": len(original_detail["rules"]),
        },
        "ports": {
            "http_proxy": http_proxy_port,
            "socket_proxy": socket_proxy_port,
            "http_mock": http_upstream_port,
            "socket_mock": socket_upstream_port,
        },
    }
    failure: BaseException | None = None
    try:
        applied = apply_candidate(
            candidate(
                workspace_id,
                original_detail["revision"],
                [
                    http_listener(http_proxy_port, http_upstream_port),
                    socket_listener(socket_proxy_port, socket_upstream_port),
                ],
            ),
            10,
        )
        test_applied = True
        statuses = wait_for_listeners(2, timeout_seconds=120)
        configured = mcp_tool("workspace_get", {"workspace_id": workspace_id}, 41)
        http_result = run_http(http_proxy_port)
        socket_result = run_socket(socket_proxy_port)
        captures = mcp_tool(
            "http_capture_query",
            {"page": 1, "page_size": 20, "direction": "desc"},
            42,
        )
        observations = mcp_tool(
            "exchange_observation_query",
            {
                "workspace_id": workspace_id,
                "listener_id": None,
                "page": {"page": 1, "page_size": 50},
            },
            43,
        )
        result.update(
            {
                "temporary_apply": applied,
                "listener_statuses": statuses,
                "configured_revision": configured["revision"],
                "configured_listener_count": len(configured["listeners"]),
                "http": http_result,
                "http_mock_records": http_server.records,  # type: ignore[attr-defined]
                "socket": socket_result,
                "socket_mock_records_hex": [
                    value.hex() for value in tcp_server.records  # type: ignore[attr-defined]
                ],
                "http_capture_summary": {
                    "total": captures.get("total"),
                    "row_count": len(captures.get("rows", [])),
                    "rows": captures.get("rows", [])[:5],
                },
                "exchange_summary": {
                    "total": observations.get("total"),
                    "row_count": len(observations.get("rows", [])),
                    "rows": observations.get("rows", [])[:10],
                },
            }
        )
        if http_result["status"] != 201:
            raise ReplayFailure(f"unexpected HTTP status: {http_result}")
        if http_result["mock_header"] != CASE_MARKER:
            raise ReplayFailure(f"missing mock response marker: {http_result}")
        if http_result["body"]["received"]["marker"] != CASE_MARKER:
            raise ReplayFailure(f"mock server did not receive marker: {http_result}")
        if socket_result["received_hex"] != socket_result["expected_hex"]:
            raise ReplayFailure(f"Socket relay bytes differ: {socket_result}")
        if not http_server.records or not tcp_server.records:  # type: ignore[attr-defined]
            raise ReplayFailure("mock servers did not receive both requests")
        result["data_plane_result"] = "PASS"
        print("READY_FOR_LISTENER_STOP", flush=True)
        stopped_statuses = wait_for_listeners(0, timeout_seconds=240)
        result["manual_stop_status_count"] = len(stopped_statuses)
    except BaseException as error:  # preserve exact failure, then restore
        failure = error
    finally:
        if test_applied:
            try:
                current = mcp_tool("workspace_get", {"workspace_id": workspace_id}, 50)
                restore_result = apply_candidate(
                    candidate(workspace_id, current["revision"], []), 51
                )
                empty_statuses = wait_for_listeners(0)
                restored = mcp_tool("workspace_get", {"workspace_id": workspace_id}, 54)
                result["restore"] = {
                    **restore_result,
                    "listener_status_count": len(empty_statuses),
                    "workspace_id_unchanged": restored["id"] == workspace_id,
                    "workspace_name_unchanged": restored["name"] == original_detail["name"],
                    "listener_count": len(restored["listeners"]),
                    "rule_count": len(restored["rules"]),
                    "protocol_rule_count": len(restored["protocol_rules"]),
                    "android_profile_count": len(restored["android_network_profiles"]),
                }
            except BaseException as restore_error:
                failure = ReplayFailure(
                    f"test failure={failure!r}; restore failure={restore_error!r}"
                )
        http_server.shutdown()
        tcp_server.shutdown()
        http_server.server_close()
        tcp_server.server_close()

    result["result"] = "FAIL" if failure else "PASS"
    if failure:
        result["failure"] = repr(failure)
    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    if failure:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
