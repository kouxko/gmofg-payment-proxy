#!/usr/bin/env python3
"""Release-App data-plane acceptance for the Deno and AU EFTEX external packages.

The installer only inserts/replaces one dedicated E2E Workspace. It refuses to
run while the App is active, validates both exact package registrations, and
preserves every non-E2E Workspace. The data-plane runner uses real localhost TCP
App/Server sockets; it never calls package hooks directly.

Usage:
  python3 scripts/e2e_external_packages.py install
  # Start the release App and both external package processes, then start both
  # listeners in 入口配置.
  python3 scripts/e2e_external_packages.py run
  # Stop either bound package while its listener is running, then verify the
  # listener was stopped and its port released. Restarting the package must not
  # make this command fail; listeners are deliberately not auto-restarted.
  python3 scripts/e2e_external_packages.py assert-stopped deno
  python3 scripts/e2e_external_packages.py assert-stopped au-eftex
  # Stop the App, then restore the Workspace selected before `install`.
  python3 scripts/e2e_external_packages.py restore-selection --workspace-id <printed-id>
"""

from __future__ import annotations

import argparse
import json
import queue
import socket
import sqlite3
import sys
import threading
import time
from contextlib import closing
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.e2e_socket_cases import (
    ISO8583_SAMPLE_HEX,
    AcceptanceError,
    with_iso8583_message_type,
)

DATABASE_PATH = (
    Path.home()
    / "Library/Application Support/com.interceptproxy.desktop/intercept-proxy.sqlite3"
)
WORKSPACE_ID = "af8e2d21-b988-4c36-b833-b9c64a839001"
DENO_LISTENER_ID = "af8e2d21-b988-4c36-b833-b9c64a839002"
AU_EFTEX_LISTENER_ID = "af8e2d21-b988-4c36-b833-b9c64a839003"
DENO_PACKAGE = {"id": "iso8583-deno-ascii", "version": "1.0.0"}
AU_EFTEX_PACKAGE = {"id": "au-eftex", "version": "1.1.0"}
DENO_PROXY_PORT = 18083
DENO_SERVER_PORT = 19083
AU_EFTEX_PROXY_PORT = 18084
AU_EFTEX_SERVER_PORT = 19084
MCP_PORT = 17653
TIMEOUT_SECONDS = 8.0

# Public synthetic golden vectors. They contain no production key, PAN, PIN or
# transaction data. The distinct request/response ciphertexts prove that the
# external package selects the correct DUKPT direction.
AU_EFTEX_UPSTREAM_FRAME = bytes.fromhex(
    "54DF000132DF01083132333435363738DF0206303030303031"
    "DF030AFFFF9876543210E0000842"
    "7B758DDA6A29D38B8020B31687B21D636DBC15E6F3A17CDEE8A868124D4C8F84"
)
AU_EFTEX_DOWNSTREAM_FRAME = bytes.fromhex(
    "54DF000132DF01083132333435363738DF0206303030303031"
    "DF030AFFFF9876543210E0000842"
    "47737E0317A4310697A84E728F754C84798309EF10EDD18E"
)


@dataclass(frozen=True)
class InstallOutcome:
    revision: int
    backup_path: Path | None
    previous_selected_id: str


def build_workspace(revision: int) -> dict[str, Any]:
    return {
        "_persistence_version": 5,
        "id": WORKSPACE_ID,
        "name": "External Packages E2E",
        "revision": revision,
        "listeners": [
            _listener(
                DENO_LISTENER_ID,
                "E2E Deno ISO8583 18083",
                DENO_PROXY_PORT,
                DENO_SERVER_PORT,
                DENO_PACKAGE,
            ),
            _listener(
                AU_EFTEX_LISTENER_ID,
                "E2E AU EFTEX 18084",
                AU_EFTEX_PROXY_PORT,
                AU_EFTEX_SERVER_PORT,
                AU_EFTEX_PACKAGE,
            ),
        ],
        "rules": [],
        "protocol_rules": [],
        "protocol_rule_created_order_high_water": 0,
        "certificate_references": [],
        "android_network_profiles": [],
    }


def _listener(
    listener_id: str,
    name: str,
    proxy_port: int,
    server_port: int,
    package: dict[str, str],
) -> dict[str, Any]:
    return {
        "id": listener_id,
        "name": name,
        "enabled": True,
        "bind_address": "127.0.0.1",
        "port": proxy_port,
        "connect_timeout_ms": 5_000,
        "read_timeout_ms": 10_000,
        "write_timeout_ms": 10_000,
        "data_plane": {
            "kind": "socket",
            "settings": {
                "topology": {
                    "mode": "relay",
                    "settings": {
                        "upstream": {"host": "127.0.0.1", "port": server_port},
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
                    "settings": {"package": dict(package)},
                },
            },
        },
    }


def install_workspace(
    database: Path,
    *,
    backup: bool = True,
    require_app_stopped: bool = True,
) -> InstallOutcome:
    if require_app_stopped:
        _assert_app_stopped()
        for port in (DENO_PROXY_PORT, DENO_SERVER_PORT, AU_EFTEX_PROXY_PORT, AU_EFTEX_SERVER_PORT):
            _assert_port_bindable(port)
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
        for package in (DENO_PACKAGE, AU_EFTEX_PACKAGE):
            row = connection.execute(
                "SELECT enabled FROM external_protocol_packages "
                "WHERE package_id = ? AND version = ?",
                (package["id"], package["version"]),
            ).fetchone()
            identity = f"{package['id']}@{package['version']}"
            if row is None:
                raise AcceptanceError(f"Exact external package is not registered: {identity}")
            if row != (1,):
                raise AcceptanceError(f"Exact external package is disabled: {identity}")
        existing = connection.execute(
            "SELECT revision FROM workspaces WHERE id = ?", (WORKSPACE_ID,)
        ).fetchone()
        selected = connection.execute(
            "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
        ).fetchone()
        if selected is None or not isinstance(selected[0], str) or not selected[0]:
            raise AcceptanceError("App database has no selected Workspace to restore")
        revision = 1 if existing is None else int(existing[0]) + 1
        workspace = json.dumps(
            build_workspace(revision), ensure_ascii=False, separators=(",", ":")
        )
        with connection:
            connection.execute(
                "INSERT INTO workspaces(id, revision, json, updated_at) VALUES (?, ?, ?, ?) "
                "ON CONFLICT(id) DO UPDATE SET revision=excluded.revision, "
                "json=excluded.json, updated_at=excluded.updated_at",
                (WORKSPACE_ID, revision, workspace, datetime.now(UTC).isoformat()),
            )
            connection.execute(
                "UPDATE workspace_state SET selected_id = ? WHERE singleton_id = 1",
                (WORKSPACE_ID,),
            )
        return InstallOutcome(revision, backup_path, selected[0])
    finally:
        connection.close()


def _backup_database(database: Path) -> Path:
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    target = database.with_name(f"{database.stem}.before-external-e2e-{stamp}.sqlite3")
    with closing(sqlite3.connect(database, timeout=10)) as source, closing(
        sqlite3.connect(target)
    ) as destination:
        source.backup(destination)
    return target


def restore_selected_workspace(
    database: Path,
    workspace_id: str,
    *,
    require_app_stopped: bool = True,
) -> None:
    """Restore only the prior selection after the App and E2E listeners stop."""

    if require_app_stopped:
        _assert_app_stopped()
    if not database.is_file():
        raise AcceptanceError(f"App database does not exist: {database}")
    with closing(sqlite3.connect(database, timeout=10)) as connection:
        exists = connection.execute(
            "SELECT 1 FROM workspaces WHERE id = ?", (workspace_id,)
        ).fetchone()
        if exists != (1,):
            raise AcceptanceError(f"Workspace to restore does not exist: {workspace_id}")
        with connection:
            connection.execute(
                "UPDATE workspace_state SET selected_id = ? WHERE singleton_id = 1",
                (workspace_id,),
            )


def _assert_app_stopped() -> None:
    try:
        with socket.create_connection(("127.0.0.1", MCP_PORT), timeout=0.25):
            pass
    except OSError:
        return
    raise AcceptanceError(
        f"App appears active on MCP port {MCP_PORT}; stop it before installing the E2E Workspace"
    )


def _assert_port_bindable(port: int) -> None:
    try:
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", port))
    except OSError as error:
        raise AcceptanceError(f"Required E2E port {port} is occupied: {error}") from error


def _assert_fixture_selected(database: Path) -> None:
    with closing(sqlite3.connect(database, timeout=5)) as connection:
        selected = connection.execute(
            "SELECT selected_id FROM workspace_state WHERE singleton_id = 1"
        ).fetchone()
        row = connection.execute(
            "SELECT revision, json FROM workspaces WHERE id = ?", (WORKSPACE_ID,)
        ).fetchone()
    if selected != (WORKSPACE_ID,) or row is None:
        raise AcceptanceError("External E2E Workspace is not selected; run install first")
    expected = build_workspace(int(row[0]))
    if json.loads(row[1]) != expected:
        raise AcceptanceError("External E2E Workspace differs from the fixed fixture; reinstall it")


def run_acceptance(database: Path, case: str = "all") -> dict[str, Any]:
    _assert_fixture_selected(database)
    results: dict[str, Any] = {}
    if case in {"all", "deno"}:
        request = bytes.fromhex(ISO8583_SAMPLE_HEX)
        response = with_iso8583_message_type(request, "0210")
        print("\n[Deno] App → Proxy → external hooks → Server → Proxy → App")
        results["deno"] = _run_exact_roundtrip(
            DENO_PROXY_PORT, DENO_SERVER_PORT, request, response, "Deno ISO8583"
        )
    if case in {"all", "au-eftex"}:
        print("\n[AU EFTEX] public DUKPT request/response direction vectors")
        results["au_eftex"] = _run_exact_roundtrip(
            AU_EFTEX_PROXY_PORT,
            AU_EFTEX_SERVER_PORT,
            AU_EFTEX_UPSTREAM_FRAME,
            AU_EFTEX_DOWNSTREAM_FRAME,
            "AU EFTEX DUKPT",
        )
    return results


def write_evidence(target: Path, results: dict[str, Any]) -> None:
    """Write stable machine-readable L3 evidence without including payload bytes."""

    observed_at = datetime.now(UTC).isoformat()
    rows: list[dict[str, Any]] = []
    for test_id in ("deno", "au_eftex"):
        if test_id not in results:
            continue
        result = results[test_id]
        request_bytes = result.get("request_bytes")
        response_bytes = result.get("response_bytes")
        if any(type(value) is not int or value < 0 for value in (request_bytes, response_bytes)):
            raise AcceptanceError(f"{test_id} evidence byte counts must be non-negative integers")
        rows.append(
            {
                "test_id": test_id,
                "layer": "L3",
                "status": "PASS",
                "observed_at": observed_at,
                "request_bytes": request_bytes,
                "response_bytes": response_bytes,
            }
        )

    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("w", encoding="utf-8") as evidence:
        for row in rows:
            evidence.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
            evidence.write("\n")


def _run_exact_roundtrip(
    proxy_port: int,
    server_port: int,
    request: bytes,
    response: bytes,
    label: str,
) -> dict[str, Any]:
    observed: queue.Queue[bytes | BaseException] = queue.Queue(maxsize=1)
    ready = threading.Event()

    def serve() -> None:
        try:
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", server_port))
                listener.listen(1)
                listener.settimeout(TIMEOUT_SECONDS)
                ready.set()
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(TIMEOUT_SECONDS)
                    received = _receive_exact(connection, len(request))
                    observed.put(received)
                    _send_fragmented(connection, response)
        except BaseException as error:
            observed.put(error)
            ready.set()

    thread = threading.Thread(target=serve, name=f"e2e-{label}-server", daemon=True)
    thread.start()
    if not ready.wait(TIMEOUT_SECONDS):
        raise AcceptanceError(f"{label} mock Server did not become ready")
    try:
        with socket.create_connection(("127.0.0.1", proxy_port), TIMEOUT_SECONDS) as app:
            app.settimeout(TIMEOUT_SECONDS)
            _send_fragmented(app, request)
            actual_response = _receive_exact(app, len(response))
    except ConnectionRefusedError as error:
        raise AcceptanceError(f"{label} listener on {proxy_port} is not running") from error
    thread.join(TIMEOUT_SECONDS)
    server_result = observed.get(timeout=TIMEOUT_SECONDS)
    if isinstance(server_result, BaseException):
        raise AcceptanceError(f"{label} mock Server failed: {server_result}") from server_result
    _require(server_result == request, f"{label} request reached Server byte-for-byte")
    _require(actual_response == response, f"{label} response reached App byte-for-byte")
    return {"request_bytes": len(request), "response_bytes": len(response)}


def _receive_exact(connection: socket.socket, size: int) -> bytes:
    result = bytearray()
    while len(result) < size:
        chunk = connection.recv(size - len(result))
        if not chunk:
            raise AcceptanceError("Socket closed before the expected payload was complete")
        result.extend(chunk)
    return bytes(result)


def _send_fragmented(connection: socket.socket, payload: bytes) -> None:
    boundaries = sorted({1, min(9, len(payload)), min(31, len(payload)), len(payload)})
    start = 0
    for end in boundaries:
        if end > start:
            connection.sendall(payload[start:end])
            start = end
            time.sleep(0.01)


def assert_listener_stopped(package: str) -> None:
    port = DENO_PROXY_PORT if package == "deno" else AU_EFTEX_PROXY_PORT
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.5):
            pass
    except OSError:
        _assert_port_bindable(port)
        print(f"PASS  {package} listener {port} is stopped and the port is released")
        return
    raise AcceptanceError(f"{package} listener {port} is still accepting connections")


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise AcceptanceError(f"FAILED: {message}")
    print(f"PASS  {message}")


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=("install", "run", "assert-stopped", "restore-selection")
    )
    parser.add_argument("case", nargs="?", choices=("all", "deno", "au-eftex"), default="all")
    parser.add_argument("--database", type=Path, default=DATABASE_PATH)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--workspace-id")
    return parser.parse_args()


def main() -> None:
    arguments = _arguments()
    try:
        if arguments.command == "install":
            outcome = install_workspace(arguments.database)
            print(f"installed_workspace={WORKSPACE_ID}")
            print(f"revision={outcome.revision}")
            print(f"backup={outcome.backup_path}")
            print(f"previous_selected_id={outcome.previous_selected_id}")
            print("Start the release App and both external packages, then start both E2E listeners.")
        elif arguments.command == "run":
            result = run_acceptance(arguments.database, arguments.case)
            if arguments.evidence is not None:
                write_evidence(arguments.evidence, result)
                print(f"evidence={arguments.evidence}")
            print("\nRESULT: release-App external-package data plane passed")
            print(json.dumps(result, ensure_ascii=False, indent=2))
        elif arguments.command == "assert-stopped":
            if arguments.case == "all":
                raise AcceptanceError("assert-stopped requires deno or au-eftex")
            assert_listener_stopped(arguments.case)
        else:
            if not arguments.workspace_id:
                raise AcceptanceError("restore-selection requires --workspace-id")
            restore_selected_workspace(arguments.database, arguments.workspace_id)
            print(f"restored_selected_workspace={arguments.workspace_id}")
    except (AcceptanceError, OSError, sqlite3.Error, json.JSONDecodeError) as error:
        raise SystemExit(f"E2E FAILED: {error}") from None


if __name__ == "__main__":
    main()
