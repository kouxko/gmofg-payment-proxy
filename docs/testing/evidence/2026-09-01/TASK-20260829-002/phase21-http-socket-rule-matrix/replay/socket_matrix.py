#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import socket
import sys
import time
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[7]
sys.path.insert(0, str(REPO / "scripts"))

from e2e_macos_mounted_release import mcp_call  # noqa: E402

EVIDENCE = Path(__file__).resolve().parents[1]
OUTPUTS = EVIDENCE / "outputs"
INPUTS = EVIDENCE / "inputs"
MCP_PORT = 17653
PROXY_PORT = 8081
UPSTREAM_PORT = 18084
PACKAGE = {"id": "iso8583-ascii-standard", "version": "1.0.0"}


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def selected_workspace() -> tuple[str, int]:
    selected = next(item for item in mcp_call(MCP_PORT, 1, "workspace_list", {}) if item["selected"])
    return selected["id"], selected["revision"]


def candidate(workspace_id: str, revision: int) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "target": {
            "mode": "existing",
            "workspace_id": workspace_id,
            "expected_revision": revision,
        },
        "workspace": {
            "listeners": [
                {
                    "alias": "phase21-socket",
                    "name": "Phase21 Socket schema matrix",
                    "enabled": True,
                    "bind_address": "127.0.0.1",
                    "port": PROXY_PORT,
                    "connect_timeout_ms": 5_000,
                    "read_timeout_ms": 10_000,
                    "write_timeout_ms": 10_000,
                    "data_plane": {
                        "kind": "socket",
                        "settings": {
                            "topology": {
                                "mode": "relay",
                                "settings": {
                                    "upstream": {"host": "127.0.0.1", "port": UPSTREAM_PORT},
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
                }
            ],
            "rules": [
                {
                    "name": "MTI 0200 to 0100",
                    "enabled": True,
                    "priority": 10,
                    "listener_alias": "phase21-socket",
                    "stage": "proxy_to_upstream",
                    "content": {
                        "type": "socket",
                        "value": {
                            "package": PACKAGE,
                            "conditions": [
                                {
                                    "source": "document",
                                    "path": "/message_type",
                                    "predicate": {
                                        "type": "string",
                                        "value": {"operator": "equal", "value": "0200"},
                                    },
                                }
                            ],
                            "actions": [
                                {
                                    "source": "document",
                                    "value": {
                                        "type": "set",
                                        "path": "/message_type",
                                        "value": "0100",
                                    },
                                }
                            ],
                        },
                    },
                }
            ],
            "android_network_profiles": [],
        },
        "materials": {"certificates": [], "secrets": []},
    }


def apply_candidate() -> None:
    workspace_id, revision = selected_workspace()
    value = candidate(workspace_id, revision)
    write_json(INPUTS / "socket-candidate.json", value)
    created = mcp_call(MCP_PORT, 10, "environment_candidate_create", {"candidate": value})
    write_json(OUTPUTS / "socket-candidate-preview.json", created)
    if created.get("status") != "preview_ready":
        raise RuntimeError(f"preview failed: {created}")
    mcp_call(
        MCP_PORT,
        11,
        "environment_candidate_apply",
        {
            "candidate_id": created["candidate_id"],
            "confirmation_token": created["confirmation_token"],
        },
    )
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        status = mcp_call(
            MCP_PORT,
            12,
            "environment_candidate_status",
            {"candidate_id": created["candidate_id"]},
        )
        if status.get("status") == "committed":
            write_json(OUTPUTS / "socket-candidate-committed.json", status)
            readback = mcp_call(MCP_PORT, 13, "workspace_get", {"workspace_id": workspace_id})
            write_json(OUTPUTS / "socket-workspace-before-frames.json", readback)
            print(json.dumps({"workspace_id": workspace_id, "listener_id": readback["listeners"][0]["id"]}))
            return
        if status.get("status") not in ("apply_queued", "apply_in_progress"):
            raise RuntimeError(f"apply failed: {status}")
        time.sleep(0.1)
    raise RuntimeError("candidate apply timed out")


def read_frame(connection: socket.socket) -> bytes | None:
    connection.settimeout(4)
    prefix = bytearray()
    try:
        while len(prefix) < 2:
            chunk = connection.recv(2 - len(prefix))
            if not chunk:
                return None
            prefix.extend(chunk)
        length = int.from_bytes(prefix, "big")
        payload = bytearray()
        while len(payload) < length:
            chunk = connection.recv(length - len(payload))
            if not chunk:
                return None
            payload.extend(chunk)
        return bytes(prefix + payload)
    except TimeoutError:
        return None


def message_type(frame: bytes) -> str:
    return frame[2:6].decode("ascii")


def serve() -> None:
    log_path = OUTPUTS / "socket-server.jsonl"
    with socket.socket() as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", UPSTREAM_PORT))
        listener.listen(16)
        print(f"READY {UPSTREAM_PORT}", flush=True)
        while True:
            connection, address = listener.accept()
            with connection:
                frame = read_frame(connection)
                if frame is None:
                    continue
                record = {
                    "remote": f"{address[0]}:{address[1]}",
                    "frame_hex": frame.hex(),
                    "declared_length": int.from_bytes(frame[:2], "big"),
                    "message_type": message_type(frame),
                }
                with log_path.open("a") as log:
                    log.write(json.dumps(record, ensure_ascii=False) + "\n")
                connection.sendall(frame)


def send_frame(name: str, frame: bytes) -> dict[str, Any]:
    result: dict[str, Any] = {"name": name, "sent_hex": frame.hex()}
    with socket.create_connection(("127.0.0.1", PROXY_PORT), timeout=5) as connection:
        connection.settimeout(8)
        connection.sendall(frame)
        response = read_frame(connection)
        if response is None:
            result["eof"] = True
        else:
            result.update({"received_hex": response.hex(), "message_type": message_type(response)})
    write_json(OUTPUTS / f"socket-{name}-client.json", result)
    return result


def run_client() -> None:
    match = bytes.fromhex((INPUTS / "socket-match-0200.hex").read_text().strip())
    miss = bytes.fromhex((INPUTS / "socket-miss-0400.hex").read_text().strip())
    invalid = bytes.fromhex((INPUTS / "socket-invalid.hex").read_text().strip())
    match_result = send_frame("match", match)
    if match_result.get("message_type") != "0100":
        raise AssertionError(f"match should be rewritten to 0100: {match_result}")
    miss_result = send_frame("miss", miss)
    if miss_result.get("message_type") != "0400" or miss_result.get("received_hex") != miss.hex():
        raise AssertionError(f"miss should remain byte-identical: {miss_result}")
    invalid_result = send_frame("invalid", invalid)
    if not invalid_result.get("eof"):
        raise AssertionError(f"invalid frame should fail closed: {invalid_result}")
    time.sleep(0.5)
    records = [json.loads(line) for line in (OUTPUTS / "socket-server.jsonl").read_text().splitlines()]
    if [record["message_type"] for record in records] != ["0100", "0400"]:
        raise AssertionError(f"upstream frames mismatch: {records}")
    workspace_id, _ = selected_workspace()
    workspace = mcp_call(MCP_PORT, 30, "workspace_get", {"workspace_id": workspace_id})
    write_json(OUTPUTS / "socket-rules-after.json", mcp_call(MCP_PORT, 31, "workspace_rule_list", {"workspace_id": workspace_id}))
    write_json(
        OUTPUTS / "socket-exchanges-after.json",
        mcp_call(
            MCP_PORT,
            32,
            "exchange_observation_query",
            {
                "workspace_id": workspace_id,
                "listener_id": workspace["listeners"][0]["id"],
                "page": {"page": 1, "page_size": 100},
            },
        ),
    )
    summary = {"result": "PASS", "match_rewritten_to": "0100", "miss_unchanged": True, "invalid_fail_closed": True}
    write_json(OUTPUTS / "socket-summary.json", summary)
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
