#!/usr/bin/env python3
"""Controlled physical-A920 gate for Companion v2 LocalSocket/TUN/JNI recovery.

This is deliberately not a desktop end-to-end test.  It only owns the adb
forward/reverse entries and loopback fixtures that it creates.
"""

import argparse
import hashlib
import json
import re
import shlex
import socket
import struct
import subprocess
import sys
import threading
import time
import traceback
import uuid
from datetime import datetime, timezone
from pathlib import Path


COMPANION = "com.interceptproxy.vpn"
TARGET = "com.interceptproxy.vpn.targetprobe"
ISOLATION = "com.interceptproxy.vpn.isolationprobe"
ACTIVITY = "com.interceptproxy.vpn.isolationprobe.ProbeActivity"
ORIGINAL_HOST = "203.0.113.48"
ORIGINAL_PORTS = {"dll": 16127, "transaction": 16627}
DEVICE_PROXY_PORTS = {"dll": 6555, "transaction": 6556}
MAX_FRAME = 1024 * 1024


def now():
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="milliseconds")


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_json(value):
    return hashlib.sha256(canonical(value).encode("utf-8")).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_bytes_pair(base, value):
    base.parent.mkdir(parents=True, exist_ok=True)
    Path(str(base) + ".bin").write_bytes(value)
    Path(str(base) + ".hex").write_text(value.hex() + "\n", encoding="ascii")


def append_suffix(base, suffix):
    return Path(str(base) + suffix)


def controlled_response(channel):
    body = canonical({"result": "D48", "channel": channel}).encode("utf-8")
    return (b"HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=UTF-8\r\n"
            + f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode("ascii") + body)


class Adb:
    def __init__(self, serial, report):
        self.serial = serial
        self.report = report

    def run(self, *args, check=True, binary=False, timeout=30):
        command = ["adb", "-s", self.serial, *map(str, args)]
        started = time.monotonic()
        try:
            completed = subprocess.run(command, capture_output=True, timeout=timeout)
            return_code, stdout, stderr = completed.returncode, completed.stdout, completed.stderr
        except subprocess.TimeoutExpired as error:
            return_code = "TIMEOUT"
            stdout = error.stdout or b""
            stderr = error.stderr or b""
        entry = {
            "at": now(), "command": command, "exit": return_code,
            "elapsed_millis": round((time.monotonic() - started) * 1000),
            "stdout": stdout.decode("utf-8", "replace"),
            "stderr": stderr.decode("utf-8", "replace"),
        }
        self.report.setdefault("adb_commands", []).append(entry)
        if check and return_code != 0:
            raise RuntimeError(f"adb failed ({return_code}): {' '.join(command)}\n{entry['stderr']}")
        return stdout if binary else entry["stdout"]


class Fixture(threading.Thread):
    def __init__(self, channel, output):
        super().__init__(name=f"fixture-{channel}", daemon=True)
        self.channel = channel
        self.output = output
        self.ready = threading.Event()
        self.stop_event = threading.Event()
        self.port = None
        self.error = None
        self.records = []

    def run(self):
        try:
            with socket.socket() as server:
                server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                server.bind(("127.0.0.1", 0))
                self.port = server.getsockname()[1]
                server.listen(8)
                server.settimeout(0.25)
                self.ready.set()
                while not self.stop_event.is_set():
                    try:
                        client, peer = server.accept()
                    except socket.timeout:
                        continue
                    with client:
                        client.settimeout(8)
                        chunks, raw = [], b""
                        expected = None
                        while expected is None or len(raw) < expected:
                            part = client.recv(8192)
                            if not part:
                                break
                            chunks.append(part)
                            raw += part
                            if b"\r\n\r\n" in raw:
                                head, body = raw.split(b"\r\n\r\n", 1)
                                match = re.search(br"(?im)^Content-Length:\s*(\d+)\s*$", head)
                                expected = len(head) + 4 + (int(match.group(1)) if match else 0)
                        index = len(self.records) + 1
                        record_dir = self.output / self.channel / f"request-{index:02d}"
                        write_bytes_pair(record_dir / "received-request", raw)
                        write_json(record_dir / "received-chunks.json", [
                            {"index": i + 1, "size": len(chunk), "hex": chunk.hex()}
                            for i, chunk in enumerate(chunks)
                        ])
                        write_json(record_dir / "received-request.json", {
                            "channel": self.channel, "peer": list(peer), "size": len(raw),
                            "header_latin1": raw.split(b"\r\n\r\n", 1)[0].decode("latin1", "replace"),
                            "body_utf8": raw.split(b"\r\n\r\n", 1)[1].decode("utf-8", "replace")
                            if b"\r\n\r\n" in raw else None,
                        })
                        response = controlled_response(self.channel)
                        client.sendall(response)
                        write_bytes_pair(record_dir / "sent-response", response)
                        self.records.append({"request": raw, "response": response, "dir": str(record_dir)})
        except Exception as error:
            self.error = f"{type(error).__name__}: {error}"
            self.ready.set()

    def close(self):
        self.stop_event.set()
        self.join(3)
        if self.is_alive():
            raise RuntimeError(f"fixture thread did not stop: {self.channel}")
        if self.error:
            raise RuntimeError(f"fixture failed: {self.channel}: {self.error}")


class Control:
    def __init__(self, adb, output, report):
        self.adb, self.output, self.report = adb, output, report
        self.port = None
        self.index = 0
        self.lock = threading.Lock()

    def open(self):
        self.adb.run("shell", "am", "start", "-n", f"{COMPANION}/.AdbControlActivity",
                     "--es", "command", "wake_control_server")
        raw = self.adb.run("forward", "tcp:0", "localabstract:intercept_proxy_vpn").strip()
        self.port = int(raw.removeprefix("tcp:"))
        deadline = time.monotonic() + 3
        while True:
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.5):
                    return
            except OSError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.1)

    def close(self):
        if self.port is not None:
            try:
                self.adb.run("forward", "--remove", f"tcp:{self.port}")
            finally:
                self.port = None

    def call(self, operation, payload=None, expect_ok=True):
        request = {"version": 2, "request_id": str(uuid.uuid4()), "operation": operation,
                   "payload": payload or {}}
        body = canonical(request).encode("utf-8")
        frame = struct.pack(">I", len(body)) + body
        with self.lock:
            self.index += 1
            stem = self.output / "control" / f"{self.index:03d}-{operation}"
            write_bytes_pair(stem.with_name(stem.name + "-request"), frame)
            write_json(stem.with_name(stem.name + "-request.json"), request)
            with socket.create_connection(("127.0.0.1", self.port), timeout=6) as stream:
                stream.settimeout(6)
                stream.sendall(frame)
                prefix = read_exact(stream, 4)
                length = struct.unpack(">I", prefix)[0]
                if not 0 < length <= MAX_FRAME:
                    raise RuntimeError(f"invalid v2 response length: {length}")
                response_body = read_exact(stream, length)
            response_frame = prefix + response_body
            write_bytes_pair(stem.with_name(stem.name + "-response"), response_frame)
            response = json.loads(response_body)
            write_json(stem.with_name(stem.name + "-response.json"), response)
        if response.get("version") != 2 or response.get("request_id") != request["request_id"]:
            raise AssertionError(f"v2 response envelope mismatch: {response}")
        if bool(response.get("ok")) != expect_ok:
            raise AssertionError(f"unexpected {operation} result: {response}")
        return response


def read_exact(stream, count):
    result = b""
    while len(result) < count:
        part = stream.recv(count - len(result))
        if not part:
            raise EOFError(f"needed {count} bytes, received {len(result)}")
        result += part
    return result


def status_of(response):
    status = response.get("status")
    if not isinstance(status, dict):
        raise AssertionError(f"missing status object: {response}")
    return status


def assert_stopped(status, label):
    if status.get("state") != "stopped" or status.get("active_profile_id") is not None \
            or status.get("active_profile_fingerprint") is not None \
            or status.get("active_route_fingerprint") is not None \
            or status.get("active_route_count") != 0:
        raise AssertionError(f"{label} is not stopped/no-active: {status}")


def assert_running(status, profile, runtime, label):
    expected = {
        "state": "running",
        "active_profile_id": profile["id"],
        "active_profile_fingerprint": runtime["profile_fingerprint"],
        "active_route_fingerprint": runtime["route_fingerprint"],
        "active_route_count": runtime["route_count"],
    }
    mismatches = {key: {"expected": value, "actual": status.get(key)}
                  for key, value in expected.items() if status.get(key) != value}
    if mismatches:
        raise AssertionError(f"{label} runtime identity mismatch: {mismatches}")


def runtime_identity_fields(value, path="status"):
    found = {}
    if isinstance(value, dict):
        for key, child in value.items():
            child_path = f"{path}.{key}"
            if "epoch" in key.lower() or "generation" in key.lower():
                found[child_path] = child
            found.update(runtime_identity_fields(child, child_path))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            found.update(runtime_identity_fields(child, f"{path}[{index}]"))
    return found


def wait_state(control, wanted, timeout=12):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = status_of(control.call("status"))
        if last.get("state") == wanted:
            return last
        if wanted == "running" and last.get("state") in ("faulted", "stopped"):
            raise AssertionError(f"VPN failed before running: {last}")
        time.sleep(0.25)
    raise TimeoutError(f"VPN did not reach {wanted}: {last}")


def package_uid(adb, package):
    text = adb.run("shell", "cmd", "package", "list", "packages", "-U", package)
    match = re.search(rf"package:{re.escape(package)}\s+uid:(\d+)", text)
    if not match:
        raise RuntimeError(f"installed UID not found for {package}: {text}")
    return int(match.group(1))


def expected_probe_request(channel, phase):
    body = canonical({"operation": "probe"}).encode("utf-8")
    path = f"/controlled/{phase}/{channel}"
    header = (
        f"POST {path} HTTP/1.1\r\n"
        f"Host: {ORIGINAL_HOST}:{ORIGINAL_PORTS[channel]}\r\n"
        "Content-Type: application/json; charset=UTF-8\r\n"
        f"Content-Length: {len(body)}\r\n"
        "Connection: close\r\n\r\n"
    ).encode("ascii")
    return header + body


def probe(adb, package, channel, phase, expect_success, output, fixture):
    token = f"{phase}-{channel}-{uuid.uuid4().hex[:10]}"
    remote = f"files/probe-{token}.txt"
    records_before = len(fixture.records)
    adb.run("shell", "run-as", package, "rm", "-f", remote, check=False)
    shell_args = [
        "am", "start", "-n", f"{package}/{ACTIVITY}",
        "--es", "host", ORIGINAL_HOST, "--ei", "port", str(ORIGINAL_PORTS[channel]),
        "--ei", "timeout", "7000", "--es", "token", token,
        "--es", "path", f"/controlled/{phase}/{channel}", "--es", "method", "POST",
        "--es", "body", canonical({"operation": "probe"}),
    ]
    adb.run("shell", shlex.join(shell_args))
    deadline, raw, last_read = time.monotonic() + 10, None, None
    while time.monotonic() < deadline:
        result = adb.run("shell", "-T", "run-as", package, "cat", remote, check=False, binary=True)
        last_read = adb.report["adb_commands"][-1]
        if last_read["exit"] == 0 and result:
            raw = result
            break
        time.sleep(0.15)
    if raw is None:
        raise TimeoutError(f"probe result missing: {package} {phase} {channel}; last adb={last_read}")
    stem = output / "probes" / phase / f"{package}-{channel}"
    write_bytes_pair(stem, raw)
    decoded = raw.decode("utf-8", "replace")
    exact_response = controlled_response(channel)
    success = raw == exact_response
    write_json(append_suffix(stem, ".json"), {"package": package, "channel": channel,
               "phase": phase, "success": success, "text_utf8": decoded})
    if expect_success:
        if not success:
            raise AssertionError(f"probe response differs from fixture bytes: {package} {phase} {channel}")
        record_deadline = time.monotonic() + 2
        while len(fixture.records) < records_before + 1 and time.monotonic() < record_deadline:
            time.sleep(0.01)
        if len(fixture.records) != records_before + 1:
            raise AssertionError(f"fixture request count mismatch: {channel} {phase}")
        record = fixture.records[-1]
        expected_request = expected_probe_request(channel, phase)
        if record["request"] != expected_request:
            raise AssertionError(
                f"fixture request bytes differ: {channel} {phase}; "
                f"expected={expected_request.hex()} actual={record['request'].hex()}"
            )
        if raw != record["response"]:
            raise AssertionError(f"probe and fixture response bytes differ: {channel} {phase}")
        record_dir = Path(record["dir"])
        write_bytes_pair(record_dir / "expected-request", expected_request)
        write_json(record_dir / "byte-comparison.json", {
            "request_exact": record["request"] == expected_request,
            "response_exact": raw == record["response"] == exact_response,
        })
    else:
        connection_errors = (
            "ConnectException", "SocketTimeoutException", "NoRouteToHostException",
            "UnknownHostException", "Connection refused", "Network is unreachable", "timed out",
        )
        if not raw.startswith(b"ERROR:") or not any(marker in decoded for marker in connection_errors):
            raise AssertionError(f"isolation probe did not report an explicit connection failure: {decoded}")
        if len(fixture.records) != records_before:
            raise AssertionError(f"isolation probe reached fixture: {channel} {phase}")
    return {"package": package, "phase": phase, "channel": channel, "success": success,
            "bytes": len(raw), "text": decoded}


def tun_names(text):
    return sorted(set(re.findall(r"(?m)^\d+:\s+(tun\d+)(?:@[^:]+)?:", text)))


def occupied_reverse_ports(listing):
    occupied = set()
    for line in listing.splitlines():
        fields = line.split()
        local = fields[1] if len(fields) >= 3 else fields[0] if fields else None
        if local is not None and not local.startswith("tcp:"):
            raise RuntimeError(f"unrecognized adb reverse --list line: {line}")
        if local is not None:
            occupied.add(int(local.removeprefix("tcp:")))
    return occupied


def build_config(uid, fixture_ports):
    listener_ids = {name: str(uuid.uuid4()) for name in ORIGINAL_PORTS}
    weak = {
        "seed": 48, "fixed_delay_millis": 120, "uniform_jitter_millis": 30,
        "upload_bytes_per_second": None, "download_bytes_per_second": None,
        "random_loss_basis_points": 0, "burst_loss": None, "duplicate_basis_points": 0,
        "reorder_basis_points": 0, "maximum_reorder_hold_millis": 0,
        "blackout_windows": [], "dns_blackhole": False, "nth_tcp_flag_drops": [],
        "path_mtu": {"mtu": None, "mss_clamp": None, "mode": "pass"},
        "corruption": {"probability_basis_points": 0, "bits_per_packet": 0},
    }
    profile_routes = [{"destination": ORIGINAL_HOST, "ports": [ORIGINAL_PORTS[name]],
                       "listener_id": listener_ids[name]} for name in ORIGINAL_PORTS]
    route_source = [
        {"listener_id": listener_ids[name],
         "listener_name": f"A920 controlled {name} fixture",
         "original_destination": ORIGINAL_HOST,
         "original_ports": [ORIGINAL_PORTS[name]],
         "desktop_listener_bind_address": "127.0.0.1",
         "desktop_listener_port": fixture_ports[name]}
        for name in ORIGINAL_PORTS
    ]
    profile = {
        "id": "a920-controlled-v2", "name": "A920 controlled v2 recovery gate",
        "target_applications": [{"package_name": TARGET, "uid": uid,
                                  "display_name": "Intercept Proxy Target Probe"}],
        "destination_targets": [{"cidr": ORIGINAL_HOST, "ports": [ORIGINAL_PORTS["dll"]]}],
        "proxy_routes": profile_routes, "confirmed_shared_uids": [],
        "auto_resume_after_reboot": False, "stop_vpn_on_control_loss": True,
        "weak_network": weak,
    }
    routes = [{"listener_id": listener_ids[name], "original_destination": ORIGINAL_HOST,
               "original_ports": [ORIGINAL_PORTS[name]], "resolved_original_ips": [],
               "proxy_host": "127.0.0.1", "proxy_port": DEVICE_PROXY_PORTS[name]}
              for name in ORIGINAL_PORTS]
    runtime = {"routes": routes, "route_source": route_source,
               "profile_fingerprint": sha256_json(profile),
               "route_fingerprint": sha256_json(routes), "route_count": len(routes)}
    return profile, runtime


def load_recovery_gate(path, serial):
    metadata_path = path.resolve()
    if not metadata_path.is_file():
        raise RuntimeError(f"recovery backup metadata is not a file: {metadata_path}")
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    if not isinstance(metadata, dict):
        raise RuntimeError("recovery backup metadata must be a JSON object")
    expected = {
        "package": COMPANION,
        "serial": serial,
        "backup_complete": True,
        "restore_supported": True,
    }
    mismatches = {key: {"expected": value, "actual": metadata.get(key)}
                  for key, value in expected.items() if metadata.get(key) != value}
    if mismatches:
        raise RuntimeError(f"recovery backup metadata mismatch: {mismatches}")
    backup_value = metadata.get("backup_file")
    if not isinstance(backup_value, str) or not backup_value.strip():
        raise RuntimeError("recovery backup metadata requires non-empty backup_file")
    backup_file = Path(backup_value)
    if not backup_file.is_absolute():
        backup_file = metadata_path.parent / backup_file
    backup_file = backup_file.resolve()
    if not backup_file.is_file() or backup_file.stat().st_size == 0:
        raise RuntimeError(f"recovery backup file is missing or empty: {backup_file}")
    try:
        json.loads(backup_file.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"recovery backup file is not valid UTF-8 JSON: {backup_file}: {error}") from error
    return metadata_path, backup_file, metadata


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--recovery-backup", required=True, type=Path,
                        help="root-generated recovery-backup-gate.json")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {
        "scope": "device LocalSocket/TUN/JNI recovery gate only",
        "serial": args.serial, "started_at": now(), "result": "RUNNING", "steps": [],
        "saved_activation_restore": {
            "possible_via_public_v2": False,
            "reason": "accepted start/apply with proxy routes clears prior recovery; v2 has no get/export operation",
            "external_backup_required": True,
            "external_restore_owner": "caller/root harness",
        },
    }
    report_path = output / "report.json"
    try:
        metadata_path, backup_file, backup_metadata = load_recovery_gate(
            args.recovery_backup, args.serial
        )
        report["recovery_backup_gate"] = {
            "metadata_file": str(metadata_path),
            "backup_file": str(backup_file),
            "metadata": backup_metadata,
        }
    except Exception as error:
        report["result"] = "FAIL"
        report["error"] = f"{type(error).__name__}: {error}"
        report["finished_at"] = now()
        write_json(report_path, report)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 1
    adb = Adb(args.serial, report)
    control = Control(adb, output, report)
    fixtures = {name: Fixture(name, output / "fixtures") for name in ORIGINAL_PORTS}
    created_reverse = []
    heartbeat_stop = threading.Event()
    heartbeat_errors = []
    owner = str(uuid.uuid4())
    vpn_owned = False

    def step(name, value=None):
        entry = {"at": now(), "name": name, "value": value}
        report["steps"].append(entry)
        write_json(report_path, report)
        print("STEP " + json.dumps(entry, ensure_ascii=False, separators=(",", ":")), flush=True)

    def heartbeat_loop():
        while not heartbeat_stop.wait(1.0):
            try:
                control.call("heartbeat", {"owner_epoch": owner})
            except Exception as error:
                heartbeat_errors.append(f"{type(error).__name__}: {error}")
                return

    heartbeat = None
    try:
        adb.run("get-state")
        for fixture in fixtures.values():
            fixture.start()
        for fixture in fixtures.values():
            if not fixture.ready.wait(3) or fixture.error:
                raise RuntimeError(f"fixture failed to bind: {fixture.channel}: {fixture.error}")
        step("fixtures_ready", {name: fixture.port for name, fixture in fixtures.items()})

        control.open()
        initial = status_of(control.call("status"))
        assert_stopped(initial, "initial status")
        before_links = adb.run("shell", "ip", "addr", "show")
        before_tun = tun_names(before_links)
        step("initial_stopped", initial)

        existing = adb.run("reverse", "--list")
        occupied = occupied_reverse_ports(existing)
        for name, device_port in DEVICE_PROXY_PORTS.items():
            if device_port in occupied:
                raise RuntimeError(f"refusing occupied adb reverse device port {device_port}")
            adb.run("reverse", "--no-rebind", f"tcp:{device_port}", f"tcp:{fixtures[name].port}")
            created_reverse.append(device_port)
        step("reverse_created", {name: DEVICE_PROXY_PORTS[name] for name in ORIGINAL_PORTS})

        target_uid = package_uid(adb, TARGET)
        isolation_uid = package_uid(adb, ISOLATION)
        if target_uid == isolation_uid:
            raise AssertionError("target and isolation probes unexpectedly share UID")
        profile, runtime = build_config(
            target_uid, {name: fixture.port for name, fixture in fixtures.items()}
        )
        write_json(output / "profile.json", profile)
        write_json(output / "proxy-runtime.json", runtime)
        lease = {"owner_epoch": owner, "timeout_millis": 5000}
        first_owner = owner
        vpn_owned = True
        control.call("start", {"profile": profile, "proxy_runtime": runtime, "control_lease": lease})
        heartbeat = threading.Thread(target=heartbeat_loop, name="control-heartbeat", daemon=True)
        heartbeat.start()
        running = wait_state(control, "running")
        assert_running(running, profile, runtime, "phase1")
        running1_links = adb.run("shell", "ip", "addr", "show")
        running1_tun = tun_names(running1_links)
        if not set(running1_tun) - set(before_tun):
            raise AssertionError(f"running phase1 did not add a TUN interface: {before_tun} -> {running1_tun}")
        step("phase1_running", {"status": running, "owner_epoch": owner,
                                "identity_fields": runtime_identity_fields(running),
                                "tun_before": before_tun, "tun_running": running1_tun})

        phase1 = [probe(adb, TARGET, name, "phase1", True, output, fixtures[name])
                  for name in ORIGINAL_PORTS]
        isolated = probe(adb, ISOLATION, "dll", "isolation", False, output, fixtures["dll"])
        step("phase1_probes", {"target": phase1, "isolation": isolated})
        if heartbeat_errors:
            raise RuntimeError(f"heartbeat failed during phase1: {heartbeat_errors}")

        heartbeat_stop.set()
        heartbeat.join(3)
        if heartbeat.is_alive():
            raise RuntimeError("heartbeat thread did not stop")
        time.sleep(5.75)
        expired = wait_state(control, "stopped", timeout=5)
        assert_stopped(expired, "post-lease status")
        step("control_loss_stopped", expired)

        stale = control.call("apply_active", {"profile": profile, "proxy_runtime": runtime,
                             "control_lease": lease}, expect_ok=False)
        if stale.get("error_code") != "ANDROID_CONTROL_LEASE_STALE":
            raise AssertionError(f"old lease rejected with wrong error: {stale}")
        after_stale = status_of(control.call("status"))
        assert_stopped(after_stale, "post-stale status")
        after_links = adb.run("shell", "ip", "addr", "show")
        after_tun = tun_names(after_links)
        if after_tun != before_tun:
            raise AssertionError(f"apply_active old lease changed TUN interfaces: {before_tun} -> {after_tun}")
        write_json(output / "tun-interface-comparison.json", {"before": before_tun, "after_stale": after_tun})
        step("stale_apply_rejected_no_auto_restore", stale)

        owner = str(uuid.uuid4())
        heartbeat_stop = threading.Event()
        lease = {"owner_epoch": owner, "timeout_millis": 5000}
        control.call("apply", {"profile": profile, "proxy_runtime": runtime, "control_lease": lease})
        heartbeat = threading.Thread(target=heartbeat_loop, name="control-heartbeat-2", daemon=True)
        heartbeat.start()
        running2 = wait_state(control, "running")
        assert_running(running2, profile, runtime, "phase2")
        running2_links = adb.run("shell", "ip", "addr", "show")
        running2_tun = tun_names(running2_links)
        if not set(running2_tun) - set(before_tun):
            raise AssertionError(f"running phase2 did not add a TUN interface: {before_tun} -> {running2_tun}")
        phase2 = [probe(adb, TARGET, name, "phase2", True, output, fixtures[name])
                  for name in ORIGINAL_PORTS]
        if heartbeat_errors:
            raise RuntimeError(f"heartbeat failed during phase2: {heartbeat_errors}")
        identity1 = runtime_identity_fields(running)
        identity2 = runtime_identity_fields(running2)
        common_identity_paths = sorted(set(identity1) & set(identity2))
        changed_identity_paths = [path for path in common_identity_paths
                                  if identity1[path] != identity2[path]]
        if (identity1 or identity2) and not changed_identity_paths:
            raise AssertionError(
                f"status exposed generation/epoch fields but none changed: {identity1} -> {identity2}"
            )
        step("explicit_new_epoch_recovered", {
            "status": running2, "probes": phase2, "owner_epoch": owner,
            "identity_change": {"phase1": identity1, "phase2": identity2,
                                "changed_paths": changed_identity_paths,
                                "control_owner_epoch": {"phase1": first_owner, "phase2": owner}},
            "tun_before": before_tun, "tun_running": running2_tun,
        })

        heartbeat_stop.set()
        heartbeat.join(3)
        control.call("stop")
        final_status = wait_state(control, "stopped", timeout=5)
        assert_stopped(final_status, "final status")
        final_links = adb.run("shell", "ip", "addr", "show")
        final_tun = tun_names(final_links)
        if final_tun != before_tun:
            raise AssertionError(f"final stop did not restore TUN interfaces: {before_tun} -> {final_tun}")
        vpn_owned = False
        if any(len(fixture.records) != 2 for fixture in fixtures.values()):
            raise AssertionError({name: len(fixture.records) for name, fixture in fixtures.items()})
        report["result"] = "PASS"
        step("final_stopped", {"status": final_status, "tun_before": before_tun,
                               "tun_final": final_tun})
    except Exception as error:
        report["result"] = "FAIL"
        report["error"] = f"{type(error).__name__}: {error}"
        report["traceback"] = traceback.format_exc()
    finally:
        heartbeat_stop.set()
        if heartbeat is not None:
            heartbeat.join(3)
            if heartbeat.is_alive():
                report.setdefault("cleanup_errors", []).append("heartbeat thread did not stop")
                report["result"] = "FAIL"
        if vpn_owned:
            try:
                response = control.call("stop")
                report["cleanup_stop"] = response
                cleanup_status = wait_state(control, "stopped", timeout=5)
                assert_stopped(cleanup_status, "cleanup status")
                report["cleanup_status"] = cleanup_status
                if "before_tun" in locals():
                    cleanup_links = adb.run("shell", "ip", "addr", "show")
                    cleanup_tun = tun_names(cleanup_links)
                    if cleanup_tun != before_tun:
                        raise AssertionError(
                            f"cleanup stop did not restore TUN interfaces: {before_tun} -> {cleanup_tun}"
                        )
                    report["cleanup_tun"] = {"before": before_tun, "after": cleanup_tun}
            except Exception as error:
                report.setdefault("cleanup_errors", []).append(
                    f"cleanup stop/confirmation: {type(error).__name__}: {error}"
                )
                report["result"] = "FAIL"
        try:
            control.close()
        except Exception as error:
            report.setdefault("cleanup_errors", []).append(
                f"control forward removal: {type(error).__name__}: {error}"
            )
            report["result"] = "FAIL"
        for device_port in reversed(created_reverse):
            try:
                adb.run("reverse", "--remove", f"tcp:{device_port}")
            except Exception as error:
                report.setdefault("cleanup_errors", []).append(
                    f"reverse tcp:{device_port} removal: {type(error).__name__}: {error}"
                )
                report["result"] = "FAIL"
        for fixture in fixtures.values():
            try:
                fixture.close()
            except Exception as error:
                report.setdefault("cleanup_errors", []).append(f"{type(error).__name__}: {error}")
                report["result"] = "FAIL"
        report["fixture_request_counts"] = {name: len(fixture.records) for name, fixture in fixtures.items()}
        report["heartbeat_errors"] = heartbeat_errors
        report["finished_at"] = now()
        write_json(report_path, report)
        print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
