#!/usr/bin/env python3
"""Read-only VPN/TCP regression; output must be a fresh directory. Sends no application bytes."""
import argparse, datetime, json, pathlib, socket, struct, subprocess, time
p = argparse.ArgumentParser()
p.add_argument("--adb", default="adb")
p.add_argument("--serial", default="10.0.34.117:5555")
p.add_argument("--host", default="10.0.34.117")
p.add_argument("--out", required=True)
p.add_argument("--expect-vpn", choices=["running", "stopped"], required=True)
a = p.parse_args()
out = pathlib.Path(a.out)
out.mkdir(parents=True, exist_ok=False)
adb = [a.adb, "-s", a.serial]
def shell(cmd):
    return subprocess.check_output(adb + ["shell", cmd], text=True)
def status():
    port = int(subprocess.check_output(adb + ["forward", "tcp:0", "localabstract:intercept_proxy_vpn"]).strip())
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=8) as s:
            request = {"version": 2, "request_id": "vpn-local-tcp-check", "operation": "status", "payload": {}}
            data = json.dumps(request).encode()
            s.sendall(struct.pack(">I", len(data)) + data)
            def exact(n):
                result = b""
                while len(result) < n:
                    chunk = s.recv(n-len(result))
                    if not chunk:
                        raise RuntimeError("control EOF")
                    result += chunk
                return result
            size = struct.unpack(">I", exact(4))[0]
            if not 0 < size <= 1024*1024:
                raise RuntimeError("invalid frame size")
            response = json.loads(exact(size))
            if not response["ok"]:
                raise RuntimeError(response)
            return response
    finally:
        subprocess.run(adb + ["forward", "--remove", f"tcp:{port}"], check=True)
def snapshot(label):
    result = status()
    (out / f"status-{label}.json").write_text(json.dumps(result, indent=2, ensure_ascii=False))
    for name, command in {"connectivity": "dumpsys connectivity", "routes": "ip rule; ip route show table all", "listeners": "cat /proc/net/tcp; cat /proc/net/tcp6", "mode": "content query --uri content://jp.gmofg.app.launcher.provider.settings/POSLINK_COMMUNICATION_TYPE"}.items():
        (out / f"{name}-{label}.txt").write_text(shell(command))
    return result["status"]
before = snapshot("before")
trials = []
for i in range(3):
    trial = {"attempt": i+1, "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "destination": [a.host, 7788], "application_bytes_sent": 0}
    start = time.monotonic()
    try:
        with socket.create_connection((a.host, 7788), timeout=8) as s:
            trial.update(connected=True, local=s.getsockname(), peer=s.getpeername())
    except OSError as error:
        trial.update(connected=False, error=repr(error))
    trial["elapsed_seconds"] = time.monotonic()-start
    trials.append(trial)
after = snapshot("after")
result = {"expected_vpn_state": a.expect_vpn, "trials": trials, "vpn_state_matches": before["state"] == a.expect_vpn and after["state"] == a.expect_vpn, "profile_unchanged": before["active_profile_id"] == after["active_profile_id"] and before["active_profile_fingerprint"] == after["active_profile_fingerprint"], "application_protocol": "NOT_RUN", "cleanup": "sockets closed and temporary adb forward removed"}
result["pass"] = result["vpn_state_matches"] and result["profile_unchanged"] and all(t["connected"] for t in trials)
(out / "comparison.json").write_text(json.dumps(result, indent=2, ensure_ascii=False))
print(json.dumps(result, indent=2, ensure_ascii=False))
raise SystemExit(0 if result["pass"] else 1)
