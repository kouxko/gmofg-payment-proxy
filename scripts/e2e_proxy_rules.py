#!/usr/bin/env python3
"""Install and verify deterministic HTTP/Socket rules against the desktop App.

Usage:
  python3 scripts/e2e_proxy_rules.py install
  python3 scripts/e2e_proxy_rules.py run

`install` preserves existing Workspaces, adds/selects one dedicated E2E Workspace,
and creates a timestamped SQLite backup. Restart the App after installation, start
both E2E listeners in 入口配置, then execute `run`.
"""

from __future__ import annotations

import argparse
import http.client
import json
import queue
import sqlite3
import sys
import threading
from dataclasses import dataclass
from datetime import UTC, datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.e2e_socket_cases import (
    ISO8583_SAMPLE_HEX,
    AcceptanceError,
    parse_iso8583_sample,
    run_raw_transparent_case,
    run_scripted_socket_case,
    with_iso8583_amount,
    with_iso8583_message_type,
)

DATABASE_PATH = Path.home() / "Library/Application Support/com.interceptproxy.desktop/intercept-proxy.sqlite3"
WORKSPACE_ID = "af8e2d21-b988-4c36-b833-b9c64a838001"
HTTP_LISTENER_ID = "af8e2d21-b988-4c36-b833-b9c64a838002"
SOCKET_LISTENER_ID = "af8e2d21-b988-4c36-b833-b9c64a838003"
RAW_SOCKET_LISTENER_ID = "af8e2d21-b988-4c36-b833-b9c64a838004"
PACKAGE = {"id": "iso8583-ascii-standard", "version": "1.0.0"}
HTTP_PROXY_PORT = 18080
HTTP_SERVER_PORT = 19080
SOCKET_PROXY_PORT = 18081
SOCKET_SERVER_PORT = 19081
RAW_SOCKET_PROXY_PORT = 18082
RAW_SOCKET_SERVER_PORT = 19082
TIMEOUT_SECONDS = 8.0

@dataclass(frozen=True)
class InstallOutcome:
    workspace_id: str
    revision: int
    backup_path: Path | None

def build_workspace(revision: int) -> dict[str, Any]:
    return {
        "_persistence_version": 5,
        "id": WORKSPACE_ID,
        "name": "HTTP + Socket 规则 E2E",
        "revision": revision,
        "listeners": [_http_listener(), _socket_listener(), _raw_socket_listener()],
        "rules": [_http_request_rule(), _http_response_rule()],
        "protocol_rules": _socket_rules(),
        "protocol_rule_created_order_high_water": 4,
        "certificate_references": [],
        "android_network_profiles": [],
    }

def _http_listener() -> dict[str, Any]:
    return {
        "id": HTTP_LISTENER_ID,
        "name": "E2E HTTP 18080",
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": HTTP_PROXY_PORT,
        "allowed_client_cidrs": [],
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
                    "root_ca": None,
                    "maximum_cached_leaf_certificates": 256,
                },
                "downstream_tls": {
                    "enabled": False,
                    "server_identity": None,
                    "dynamic_sni_allowlist": [],
                    "client_authentication": {"mode": "disabled"},
                },
                "request_body_codec": "auto",
                "response_body_codec": "auto",
                "body_processing": {"mode": "plain"},
                "fixed_server": {
                    "upstream_url": f"http://127.0.0.1:{HTTP_SERVER_PORT}",
                    "upstream_tls": {
                        "verify_hostname": True,
                        "server_trust": None,
                        "client_identity": None,
                    },
                },
            },
        },
    }

def _socket_listener() -> dict[str, Any]:
    return {
        "id": SOCKET_LISTENER_ID,
        "name": "E2E Socket ISO8583 18081",
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": SOCKET_PROXY_PORT,
        "allowed_client_cidrs": [],
        "connect_timeout_ms": 5_000,
        "read_timeout_ms": 10_000,
        "write_timeout_ms": 10_000,
        "data_plane": {
            "kind": "socket",
            "settings": {
                "topology": {
                    "mode": "relay",
                    "settings": {
                        "upstream": {"host": "127.0.0.1", "port": SOCKET_SERVER_PORT},
                        "security": {"mode": "transparent"},
                    },
                },
                "maximum_connections": 20,
                "runtime_limits": {
                    "read_chunk_bytes": 16_384,
                    "diagnostic_event_capacity": 256,
                    "diagnostic_memory_bytes": 1_048_576,
                },
                "processing": {"mode": "scripted", "settings": {"package": PACKAGE}},
            },
        },
    }


def _raw_socket_listener() -> dict[str, Any]:
    return {
        "id": RAW_SOCKET_LISTENER_ID,
        "name": "E2E Raw Transparent 18082",
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": RAW_SOCKET_PROXY_PORT,
        "allowed_client_cidrs": [],
        "connect_timeout_ms": 5_000,
        "read_timeout_ms": 10_000,
        "write_timeout_ms": 10_000,
        "data_plane": {
            "kind": "socket",
            "settings": {
                "topology": {
                    "mode": "relay",
                    "settings": {
                        "upstream": {
                            "host": "127.0.0.1",
                            "port": RAW_SOCKET_SERVER_PORT,
                        },
                        "security": {"mode": "transparent"},
                    },
                },
                "maximum_connections": 20,
                "runtime_limits": {
                    "read_chunk_bytes": 16_384,
                    "diagnostic_event_capacity": 256,
                    "diagnostic_memory_bytes": 1_048_576,
                },
                "processing": {"mode": "direct"},
            },
        },
    }


def _http_rule(rule_id: str, name: str, stage: str, field: str, expected: str,
               amount: int, header: str, created_order: int) -> dict[str, Any]:
    return {
        "id": rule_id,
        "revision": 1,
        "name": name,
        "description": "Python E2E fixture; safe to replace by rerunning install.",
        "enabled": True,
        "priority": 10,
        "created_order": created_order,
        "channel": None,
        "stage": stage,
        "conditions": [
            {"Field": {"field": {"JsonPath": field}, "operator": {"Equals": expected}}}
        ],
        "actions": [
            {"SetJsonField": {"path": "$.amount", "value": amount}},
            {"SetHeader": {"name": header, "value": "matched"}},
        ],
        "one_shot": False,
        "hit_count": 0,
        "last_hit_at": None,
    }

def _http_request_rule() -> dict[str, Any]:
    return _http_rule(
        "af8e2d21-b988-4c36-b833-b9c64a838011", "HTTP App → Server", "Request", "$.client",
        "app", 222, "x-e2e-request", 1,
    )

def _http_response_rule() -> dict[str, Any]:
    rule = _http_rule(
        "af8e2d21-b988-4c36-b833-b9c64a838012", "HTTP Server → App", "Response", "$.server",
        "mock", 333, "x-e2e-response", 2,
    )
    rule["actions"].append({"CustomHttpStatus": {"status": 209}})
    return rule

def _socket_rules() -> list[dict[str, Any]]:
    stages = [
        ("app_to_proxy", 1000, 1111),
        ("proxy_to_upstream", 1111, 2222),
        ("upstream_to_proxy", 2222, 3333),
        ("proxy_to_app", 3333, 4444),
    ]
    rules = []
    for created_order, (stage, expected, replacement) in enumerate(stages, start=1):
        rules.append({
            "rule_id": f"af8e2d21-b988-4c36-b833-b9c64a83802{created_order}",
            "revision": 1,
            "name": f"Socket {stage}: {expected} → {replacement}",
            "enabled": True,
            "priority": 10,
            "created_order": created_order,
            "listener_id": SOCKET_LISTENER_ID,
            "package": PACKAGE,
            "schema_version": 1,
            "stage": stage,
            "conditions": [{
                "operator": "equals",
                "field": "amount",
                "value": {"type": "int", "value": expected},
            }],
            "actions": [{
                "type": "set_field",
                "field": "amount",
                "value": {"type": "int", "value": replacement},
            }],
        })
    return rules

def install_workspace(database: Path, *, backup: bool = True) -> InstallOutcome:
    if not database.is_file():
        raise AcceptanceError(f"App database does not exist: {database}")
    backup_path = _backup_database(database) if backup else None
    connection = sqlite3.connect(database, timeout=10)
    try:
        connection.execute("PRAGMA foreign_keys = ON")
        version = connection.execute(
            "SELECT version FROM application_schema WHERE singleton_id = 1"
        ).fetchone()
        if version != (19,):
            raise AcceptanceError(f"Expected database schema 19, found {version}")
        package = connection.execute(
            "SELECT enabled, validation_state FROM protocol_packages "
            "WHERE package_id = ? AND version = ?",
            (PACKAGE["id"], PACKAGE["version"]),
        ).fetchone()
        if package is None or package[1] != "valid":
            raise AcceptanceError("Built-in ISO8583 package 1.0.0 is missing or invalid")
        existing = connection.execute(
            "SELECT revision FROM workspaces WHERE id = ?", (WORKSPACE_ID,)
        ).fetchone()
        revision = 1 if existing is None else int(existing[0]) + 1
        workspace_json = json.dumps(
            build_workspace(revision), ensure_ascii=False, separators=(",", ":")
        )
        updated_at = datetime.now(UTC).isoformat()
        with connection:
            connection.execute(
                "UPDATE protocol_packages SET enabled = 1 WHERE package_id = ? AND version = ?",
                (PACKAGE["id"], PACKAGE["version"]),
            )
            connection.execute(
                "INSERT INTO workspaces(id, revision, json, updated_at) VALUES (?, ?, ?, ?) "
                "ON CONFLICT(id) DO UPDATE SET revision=excluded.revision, "
                "json=excluded.json, updated_at=excluded.updated_at",
                (WORKSPACE_ID, revision, workspace_json, updated_at),
            )
            connection.execute(
                "UPDATE workspace_state SET selected_id = ? WHERE singleton_id = 1",
                (WORKSPACE_ID,),
            )
        return InstallOutcome(WORKSPACE_ID, revision, backup_path)
    finally:
        connection.close()

def _backup_database(database: Path) -> Path:
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    target = database.with_name(f"{database.stem}.before-e2e-{stamp}.sqlite3")
    source_connection = sqlite3.connect(database, timeout=10)
    target_connection = sqlite3.connect(target)
    try:
        source_connection.backup(target_connection)
    finally:
        target_connection.close()
        source_connection.close()
    return target

def _assert_fixture_selected(database: Path) -> None:
    connection = sqlite3.connect(database, timeout=5)
    try:
        selected = connection.execute(
            "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
        ).fetchone()
        row = connection.execute(
            "SELECT json FROM workspaces WHERE id = ?", (WORKSPACE_ID,)
        ).fetchone()
    finally:
        connection.close()
    if selected != (WORKSPACE_ID,) or row is None:
        raise AcceptanceError("E2E Workspace is not selected; run the install command first")
    workspace = json.loads(row[0])
    expected = build_workspace(int(workspace["revision"]))
    _copy_runtime_rule_state(expected, workspace)
    if workspace != expected:
        raise AcceptanceError("E2E Workspace differs from the expected fixture; reinstall it")

def _copy_runtime_rule_state(expected: dict[str, Any], actual: dict[str, Any]) -> None:
    """Ignore only counters that a successful E2E run is expected to mutate.

    Rules are paired by stable ID rather than list position, so a reordered, missing or added rule
    remains configuration drift and still stops the acceptance run.
    """
    actual_rules = actual.get("rules")
    expected_rules = expected.get("rules")
    if not isinstance(actual_rules, list) or not isinstance(expected_rules, list):
        return
    actual_by_id = {
        rule.get("id"): rule
        for rule in actual_rules
        if isinstance(rule, dict) and isinstance(rule.get("id"), str)
    }
    for expected_rule in expected_rules:
        if not isinstance(expected_rule, dict):
            continue
        actual_rule = actual_by_id.get(expected_rule.get("id"))
        if not isinstance(actual_rule, dict):
            continue
        for field in ("hit_count", "last_hit_at"):
            if field in actual_rule:
                expected_rule[field] = actual_rule[field]

class _HttpMockHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    observations: queue.Queue[dict[str, Any]]

    def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler contract
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        parsed = json.loads(body)
        self.observations.put({"path": self.path, "headers": self.headers, "json": parsed})
        response = json.dumps(
            {"server": "mock", "amount": parsed["amount"], "trace": "HTTP-E2E"},
            separators=(",", ":"),
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def log_message(self, _format: str, *args: object) -> None:
        return

def _run_http_case() -> dict[str, Any]:
    observations: queue.Queue[dict[str, Any]] = queue.Queue(maxsize=1)
    handler = type("HttpMockHandler", (_HttpMockHandler,), {"observations": observations})
    server = ThreadingHTTPServer(("127.0.0.1", HTTP_SERVER_PORT), handler)
    server_thread = threading.Thread(target=server.serve_forever, name="e2e-http-server", daemon=True)
    server_thread.start()
    try:
        connection = http.client.HTTPConnection("127.0.0.1", HTTP_PROXY_PORT, timeout=TIMEOUT_SECONDS)
        body = json.dumps({"client": "app", "amount": 111, "trace": "HTTP-E2E"}).encode()
        try:
            connection.request(
                "POST", "/e2e/http", body=body,
                headers={"Content-Type": "application/json", "Content-Length": str(len(body))},
            )
            response = connection.getresponse()
            response_body = json.loads(response.read())
            response_status = response.status
            response_header = response.getheader("x-e2e-response")
        finally:
            connection.close()
        upstream = observations.get(timeout=TIMEOUT_SECONDS)
    except ConnectionRefusedError as error:
        raise AcceptanceError(
            f"HTTP proxy 127.0.0.1:{HTTP_PROXY_PORT} is not running; start the E2E HTTP listener"
        ) from error
    finally:
        server.shutdown()
        server.server_close()
        server_thread.join(timeout=2)
    _require(upstream["path"] == "/e2e/http", "HTTP Server received the original path")
    _require(upstream["json"]["amount"] == 222, "HTTP request JSON rule changed amount to 222")
    _require(upstream["headers"].get("x-e2e-request") == "matched", "HTTP request Header rule matched")
    _require(response_status == 209, "HTTP response status rule changed status to 209")
    _require(response_body["amount"] == 333, "HTTP response JSON rule changed amount to 333")
    _require(response_header == "matched", "HTTP response Header rule matched")
    return {"server_received_amount": 222, "app_received_amount": 333, "status": 209}

def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AcceptanceError(f"FAILED: {message}")
    print(f"PASS  {message}")


def run_acceptance(database: Path) -> dict[str, Any]:
    _assert_fixture_selected(database)
    print("\n[HTTP] App → Proxy → Server → Proxy → App")
    http_result = _run_http_case()
    print("\n[Socket] fragmented ISO8583 App → Proxy → Server → Proxy → App")
    socket_result = run_scripted_socket_case(
        proxy_port=SOCKET_PROXY_PORT,
        server_port=SOCKET_SERVER_PORT,
        timeout_seconds=TIMEOUT_SECONDS,
    )
    print("\n[Raw Transparent] arbitrary bytes and directional half-close")
    raw_result = run_raw_transparent_case(
        proxy_port=RAW_SOCKET_PROXY_PORT,
        server_port=RAW_SOCKET_SERVER_PORT,
        timeout_seconds=TIMEOUT_SECONDS,
    )
    return {"http": http_result, "socket": socket_result, "raw_transparent": raw_result}


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("install", "run"))
    parser.add_argument("--database", type=Path, default=DATABASE_PATH)
    return parser.parse_args()


def main() -> None:
    arguments = _arguments()
    try:
        if arguments.command == "install":
            outcome = install_workspace(arguments.database)
            print(f"installed_workspace={outcome.workspace_id}")
            print(f"revision={outcome.revision}")
            print(f"backup={outcome.backup_path}")
            print("Restart the App, start all three E2E listeners, then run this script with 'run'.")
        else:
            result = run_acceptance(arguments.database)
            print("\nRESULT: HTTP, scripted Socket and raw transparent acceptance passed")
            print(json.dumps(result, ensure_ascii=False, indent=2))
    except (AcceptanceError, OSError, sqlite3.Error, json.JSONDecodeError) as error:
        raise SystemExit(f"E2E FAILED: {error}") from None


if __name__ == "__main__":
    main()
