from pathlib import Path
import datetime
import json
import os
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[7]
CASE = Path(__file__).resolve().parents[1]
COMMANDS = [
    ("workflow-contract", ["deno", "test", "-A", "scripts/windows-build-only-workflow.test.mjs"]),
    ("bindings", ["deno", "task", "check:bindings"]),
    ("architecture", ["deno", "task", "scan:architecture"]),
    ("source-size", ["deno", "task", "scan:source-size"]),
    ("lint", ["deno", "task", "lint"]),
    ("typecheck", ["deno", "task", "typecheck"]),
    ("frontend-coverage", ["deno", "task", "check:coverage:frontend", "--maxWorkers=2"]),
    ("rust-fmt", ["deno", "task", "check:rust:fmt"]),
    ("rust-clippy", ["deno", "task", "check:rust:clippy"]),
    ("rust-workspace", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--workspace", "--all-targets", "--all-features"]),
]
env = os.environ.copy()
env["CARGO_BUILD_JOBS"] = "2"
results = []
for name, command in COMMANDS:
    started = datetime.datetime.now().astimezone().isoformat()
    print("START", name, started, flush=True)
    log = CASE / "outputs" / (name + ".log")
    with log.open("w") as stream:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
    results.append({"case": name, "command": command, "started_at": started, "completed_at": datetime.datetime.now().astimezone().isoformat(), "exit_code": result.returncode, "log": str(log.relative_to(CASE))})
    (CASE / "outputs/quality-results.json").write_text(json.dumps(results, indent=2) + "\n")
    print("END", name, result.returncode, flush=True)
changed = []
with tarfile.open(CASE / "resources/current-inputs.tar.gz") as archive:
    members = [member for member in archive if member.isfile()]
    for member in members:
        current = ROOT / member.name
        if not current.is_file() or current.read_bytes() != archive.extractfile(member).read():
            changed.append(member.name)
(CASE / "outputs/source-stability.json").write_text(json.dumps({"files": len(members), "changed": changed, "checked_at": datetime.datetime.now().astimezone().isoformat()}, indent=2) + "\n")
print("SOURCE_CHANGED", changed, flush=True)
sys.exit(0 if not changed and all(item["exit_code"] == 0 for item in results) else 1)
