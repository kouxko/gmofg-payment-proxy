from pathlib import Path
import datetime
import json
import os
import plistlib
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[7]
CASE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from e2e_macos_mounted_release import require_free_port, wait_for_mcp, quit_app

OUT = CASE / "outputs/app-session"
OUT.mkdir()
DMG = ROOT / "docs/testing/evidence/2026-09-07/TASK-20260829-002/wasm-universal-build/outputs/artifacts/Intercept Proxy_1.0.0_universal.dmg"
for port in (17653, 28080, 28081, 28082, 28083):
    require_free_port(port)
quality = json.loads((CASE / "outputs/quality-results.json").read_text())
assert len(quality) == 10 and all(result["exit_code"] == 0 for result in quality)
mount = None
process = None
profile = Path(tempfile.mkdtemp(prefix="intercept-local-regression-"))
state = {"profile": str(profile), "dmg": str(DMG), "starts": [], "cleanup": "PENDING"}

def save():
    (OUT / "session.json").write_text(json.dumps(state, indent=2) + "\n")

def start(executable):
    global process
    environment = os.environ.copy()
    environment["HOME"] = str(profile)
    environment["CFFIXED_USER_HOME"] = str(profile)
    number = len(state["starts"]) + 1
    with (OUT / f"app-start-{number}.log").open("w") as log:
        process = subprocess.Popen([str(executable)], env=environment, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    wait_for_mcp(17653, process, 30)
    database = profile / "Library/Application Support/com.interceptproxy.desktop/intercept-proxy.sqlite3"
    assert database.is_file(), str(database)
    state["starts"].append({"pid": process.pid, "at": datetime.datetime.now().astimezone().isoformat(), "database": str(database)})
    save()
    print("APP_READY", process.pid, flush=True)

try:
    attached = subprocess.run(["hdiutil", "attach", "-plist", "-readonly", "-nobrowse", str(DMG)], check=True, capture_output=True)
    mount = next(Path(item["mount-point"]) for item in plistlib.loads(attached.stdout)["system-entities"] if "mount-point" in item)
    app = next(mount.glob("*.app"))
    executable = app / "Contents/MacOS/intercept-proxy"
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    state.update(mount=str(mount), executable=str(executable))
    start(executable)
    deadline = time.monotonic() + 1800
    while not (OUT / "stop").exists():
        if process.poll() is not None:
            raise RuntimeError(f"App exited unexpectedly: {process.returncode}")
        if (OUT / "restart").exists():
            (OUT / "restart").rename(OUT / f"restart-request-{len(state['starts'])}")
            quit_app(process, 20)
            process = None
            start(executable)
        if time.monotonic() >= deadline:
            raise TimeoutError("Local regression session exceeded 30 minutes")
        time.sleep(0.2)
    quit_app(process, 20)
    process = None
finally:
    if process is not None and process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=20)
        state["cleanup_forced_own_process"] = True
    if mount is not None:
        subprocess.run(["hdiutil", "detach", str(mount)], check=True)
    # Preserve the isolated profile outside the repository for diagnosis; never touch the normal profile.
    state["cleanup"] = "APP_STOPPED_DMG_DETACHED_PROFILE_RETAINED"
    state["completed_at"] = datetime.datetime.now().astimezone().isoformat()
    save()
    print(state["cleanup"], flush=True)
