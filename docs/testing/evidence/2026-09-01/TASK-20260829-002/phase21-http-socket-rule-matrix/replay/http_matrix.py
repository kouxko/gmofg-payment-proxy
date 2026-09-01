#!/usr/bin/env python3
from __future__ import annotations

import argparse
import http.client
import json
import socket
import sys
import threading
import time
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[7]
sys.path.insert(0, str(REPO / "scripts"))

from e2e_macos_mounted_release import mcp_call  # noqa: E402

EVIDENCE = Path(__file__).resolve().parents[1]
OUTPUTS = EVIDENCE / "outputs"
INPUTS = EVIDENCE / "inputs"
RESOURCES = EVIDENCE / "resources"
MCP_PORT = 17653
PROXY_PORT = 8080
UPSTREAM_PORT = 18083


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def selected_workspace() -> tuple[str, int]:
    workspaces = mcp_call(MCP_PORT, 1, "workspace_list", {})
    selected = next(item for item in workspaces if item["selected"])
    return selected["id"], selected["revision"]


def http_listener() -> dict[str, Any]:
    return {
        "alias": "phase21-http",
        "name": "Phase21 HTTP rule matrix",
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": PROXY_PORT,
        "connect_timeout_ms": 5_000,
        "read_timeout_ms": 10_000,
        "write_timeout_ms": 10_000,
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
                "request_body_codec": "auto",
                "response_body_codec": "auto",
                "body_processing": {"mode": "plain"},
                "fixed_server": {
                    "upstream_url": f"http://127.0.0.1:{UPSTREAM_PORT}",
                    "upstream_tls": {
                        "verify_hostname": True,
                        "server_trust_alias": None,
                        "client_identity_alias": None,
                    },
                },
            },
        },
    }


def rule(name: str, priority: int, condition: dict[str, Any], action: dict[str, Any]) -> dict[str, Any]:
    return {
        "name": name,
        "enabled": True,
        "priority": priority,
        "listener_alias": "phase21-http",
        "stage": "proxy_to_upstream",
        "content": {
            "type": "http",
            "value": {
                "description": name,
                "conditions": [condition],
                "actions": [action],
            },
        },
    }


def candidate(workspace_id: str, revision: int) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "target": {
            "mode": "existing",
            "workspace_id": workspace_id,
            "expected_revision": revision,
        },
        "workspace": {
            "listeners": [http_listener()],
            "rules": [
                rule(
                    "Method POST",
                    10,
                    {"source": "http", "field": "Method", "operator": {"Equals": "POST"}},
                    {
                        "source": "http",
                        "value": {"SetHeader": {"name": "X-Method-Hit", "value": "yes"}},
                    },
                ),
                rule(
                    "Header X-Probe alpha",
                    11,
                    {
                        "source": "http",
                        "field": {"Header": "/x-probe"},
                        "operator": {"Equals": "alpha"},
                    },
                    {
                        "source": "http",
                        "value": {"SetHeader": {"name": "X-Header-Hit", "value": "yes"}},
                    },
                ),
                rule(
                    "Path and query wildcard",
                    12,
                    {
                        "source": "http",
                        "field": "RequestTarget",
                        "operator": {"Wildcard": "/orders/*?mode=test"},
                    },
                    {
                        "source": "http",
                        "value": {"SetHeader": {"name": "X-Path-Hit", "value": "yes"}},
                    },
                ),
                rule(
                    "Body age 18",
                    13,
                    {
                        "source": "document",
                        "path": "/customer/age",
                        "predicate": {"type": "number", "value": {"operator": "equal", "value": 18}},
                    },
                    {
                        "source": "document",
                        "value": {
                            "type": "set",
                            "path": "/customer/name",
                            "value": "matched",
                        },
                    },
                ),
            ],
            "android_network_profiles": [],
        },
        "materials": {"certificates": [], "secrets": []},
    }


def apply_candidate() -> None:
    workspace_id, revision = selected_workspace()
    value = candidate(workspace_id, revision)
    write_json(INPUTS / "http-candidate.json", value)
    created = mcp_call(MCP_PORT, 10, "environment_candidate_create", {"candidate": value})
    write_json(OUTPUTS / "http-candidate-preview.json", created)
    if created.get("status") != "preview_ready":
        raise RuntimeError(f"preview failed: {created}")
    queued = mcp_call(
        MCP_PORT,
        11,
        "environment_candidate_apply",
        {
            "candidate_id": created["candidate_id"],
            "confirmation_token": created["confirmation_token"],
        },
    )
    write_json(OUTPUTS / "http-candidate-queued.json", queued)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        status = mcp_call(
            MCP_PORT,
            12,
            "environment_candidate_status",
            {"candidate_id": created["candidate_id"]},
        )
        if status.get("status") == "committed":
            write_json(OUTPUTS / "http-candidate-committed.json", status)
            readback = mcp_call(MCP_PORT, 13, "workspace_get", {"workspace_id": workspace_id})
            write_json(OUTPUTS / "http-workspace-before-requests.json", readback)
            listener = readback["listeners"][0]
            print(json.dumps({"workspace_id": workspace_id, "listener_id": listener["id"]}))
            return
        if status.get("status") not in ("apply_queued", "apply_in_progress"):
            raise RuntimeError(f"apply failed: {status}")
        time.sleep(0.1)
    raise RuntimeError("candidate apply timed out")


def recv_http(connection: socket.socket) -> tuple[bytes, bytes] | None:
    connection.settimeout(2)
    raw = bytearray()
    try:
        while b"\r\n\r\n" not in raw:
            chunk = connection.recv(4096)
            if not chunk:
                return None
            raw.extend(chunk)
    except TimeoutError:
        return None
    head, body = bytes(raw).split(b"\r\n\r\n", 1)
    length = 0
    for line in head.split(b"\r\n")[1:]:
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    while len(body) < length:
        chunk = connection.recv(length - len(body))
        if not chunk:
            break
        body += chunk
    return head, body[:length]


def serve() -> None:
    log_path = OUTPUTS / "http-server.jsonl"
    with socket.socket() as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", UPSTREAM_PORT))
        listener.listen(16)
        print(f"READY {UPSTREAM_PORT}", flush=True)
        while True:
            connection, address = listener.accept()
            with connection:
                parsed = recv_http(connection)
                if parsed is None:
                    continue
                head, body = parsed
                lines = head.split(b"\r\n")
                method, target, _ = lines[0].decode("latin-1").split(" ", 2)
                headers: dict[str, list[str]] = {}
                for line in lines[1:]:
                    name, value = line.split(b":", 1)
                    headers.setdefault(name.decode("latin-1").lower(), []).append(
                        value.decode("latin-1").strip()
                    )
                try:
                    document = json.loads(body)
                except json.JSONDecodeError:
                    document = None
                record = {
                    "remote": f"{address[0]}:{address[1]}",
                    "method": method,
                    "target": target,
                    "headers": headers,
                    "body_hex": body.hex(),
                    "body_text": body.decode("utf-8", errors="replace"),
                    "document": document,
                }
                with log_path.open("a") as log:
                    log.write(json.dumps(record, ensure_ascii=False) + "\n")
                response = json.dumps(record, ensure_ascii=False).encode()
                connection.sendall(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
                    + str(len(response)).encode()
                    + b"\r\nConnection: close\r\n\r\n"
                    + response
                )


def send_case(name: str, method: str, target: str, body: bytes, headers: dict[str, str]) -> dict[str, Any]:
    connection = http.client.HTTPConnection("127.0.0.1", PROXY_PORT, timeout=8)
    result: dict[str, Any] = {"name": name, "method": method, "target": target}
    try:
        connection.request(
            method,
            target,
            body=body,
            headers={"Content-Type": "application/json", **headers},
        )
        response = connection.getresponse()
        raw = response.read()
        result.update({"status": response.status, "response": json.loads(raw)})
    except BaseException as error:
        result.update({"error_type": type(error).__name__, "error": str(error)})
    finally:
        connection.close()
    write_json(OUTPUTS / f"http-{name}-client.json", result)
    return result


def assert_header(response: dict[str, Any], name: str, present: bool) -> None:
    values = response["response"]["headers"].get(name.lower(), [])
    if present and values != ["yes"]:
        raise AssertionError(f"{name} expected yes, got {values}")
    if not present and values:
        raise AssertionError(f"{name} should be absent, got {values}")


def run_client() -> None:
    cases = [
        ("method", "POST", "/other", {"customer": {"age": 17, "name": "original"}}, {}),
        ("header", "GET", "/other", {"customer": {"age": 17, "name": "original"}}, {"X-Probe": "alpha"}),
        ("path", "GET", "/orders/123?mode=test", {"customer": {"age": 17, "name": "original"}}, {}),
        ("body", "GET", "/other", {"customer": {"age": 18, "name": "original"}}, {}),
        ("all", "POST", "/orders/123?mode=test", {"customer": {"age": 18, "name": "original"}}, {"X-Probe": "alpha"}),
        ("miss", "GET", "/other", {"customer": {"age": 17, "name": "original"}}, {}),
    ]
    results = []
    for name, method, target, document, headers in cases:
        result = send_case(name, method, target, json.dumps(document).encode(), headers)
        if result.get("status") != 200:
            raise AssertionError(f"{name} expected HTTP 200: {result}")
        assert_header(result, "X-Method-Hit", name in ("method", "all"))
        assert_header(result, "X-Header-Hit", name in ("header", "all"))
        assert_header(result, "X-Path-Hit", name in ("path", "all"))
        actual_name = result["response"]["document"]["customer"]["name"]
        expected_name = "matched" if name in ("body", "all") else "original"
        if actual_name != expected_name:
            raise AssertionError(f"{name} body action: expected {expected_name}, got {actual_name}")
        results.append(result)
    invalid = send_case("invalid", "POST", "/other", b"{", {})
    if "status" in invalid:
        raise AssertionError(f"invalid JSON unexpectedly reached upstream: {invalid}")
    time.sleep(0.5)
    server_records = [json.loads(line) for line in (OUTPUTS / "http-server.jsonl").read_text().splitlines()]
    if len(server_records) != 6:
        raise AssertionError(f"upstream expected exactly 6 requests, got {len(server_records)}")
    workspace_id, _ = selected_workspace()
    write_json(OUTPUTS / "http-rules-after.json", mcp_call(MCP_PORT, 30, "workspace_rule_list", {"workspace_id": workspace_id}))
    write_json(OUTPUTS / "http-captures-after.json", mcp_call(MCP_PORT, 31, "http_capture_query", {"page": 1, "page_size": 100}))
    workspace = mcp_call(MCP_PORT, 32, "workspace_get", {"workspace_id": workspace_id})
    write_json(
        OUTPUTS / "http-exchanges-after.json",
        mcp_call(
            MCP_PORT,
            33,
            "exchange_observation_query",
            {
                "workspace_id": workspace_id,
                "listener_id": workspace["listeners"][0]["id"],
                "page": {"page": 1, "page_size": 100},
            },
        ),
    )
    summary = {"result": "PASS", "valid_cases": len(results), "invalid_fail_closed": True}
    write_json(OUTPUTS / "http-summary.json", summary)
    print(json.dumps(summary))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("server", "prepare", "client"))
    args = parser.parse_args()
    if args.mode == "server":
        serve()
    elif args.mode == "prepare":
        apply_candidate()
    else:
        run_client()


if __name__ == "__main__":
    main()
