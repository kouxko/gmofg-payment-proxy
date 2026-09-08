#!/usr/bin/env python3
"""Run one validation command, preserving output and tested source identity."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from zoneinfo import ZoneInfo

case = Path(__file__).resolve().parents[1]
repo = Path(__file__).resolve().parents[7]
label, *command = sys.argv[1:]
if command and command[0] == "--":
    command.pop(0)
if not command or not label.replace("-", "").replace("_", "").isalnum():
    raise SystemExit("usage: run.py <unique-label> -- <command> [args...]")
output = case / "outputs" / label
output.mkdir(parents=True, exist_ok=False)
prefixes = tuple(os.environ.get(
    "INTEGER_JSON_SOURCE_PREFIXES", "src/,src-tauri/,examples/,scripts/"
).split(","))


def now():
    return datetime.datetime.now(ZoneInfo("Asia/Shanghai")).isoformat()


def sources():
    paths = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=repo
    ).decode().split("\0")
    return {
        name: hashlib.sha256((repo / name).read_bytes()).hexdigest()
        for name in sorted(set(paths))
        if name and (repo / name).is_file()
        and name.startswith(prefixes)
        and "/target/" not in name
        and (repo / name).suffix in (".rs", ".json", ".toml", ".lock", ".ts", ".tsx", ".mjs", ".wasm", ".wit", ".java", ".jar", ".sh", ".prefix", ".suffix")
    }


before = sources()
metadata = {
    "task_id": "TASK-20260908-001", "case_id": "integer-json", "label": label,
    "command": command, "cwd": str(repo), "started_at": now(),
    "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo).decode().strip(),
    "branch": subprocess.check_output(["git", "branch", "--show-current"], cwd=repo).decode().strip(),
    "source_before": before,
    "source_prefixes": prefixes,
    "environment_overrides": {k: os.environ[k] for k in ("CPS_WIRE_EVIDENCE_DIR", "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "INTEGER_JSON_SOURCE_PREFIXES", "CPS_REPO", "GSON_JAR", "FIXTURE", "JBR_HOME") if k in os.environ},
}
with (output / "stdout.txt").open("wb") as stdout, (output / "stderr.txt").open("wb") as stderr:
    result = subprocess.run(command, cwd=repo, stdout=stdout, stderr=stderr, check=False)
after = sources()
metadata.update({
    "finished_at": now(), "exit_code": result.returncode,
    "source_after": after, "source_stable": before == after,
    "changed_during_run": sorted(k for k in before.keys() | after.keys() if before.get(k) != after.get(k)),
})
(output / "run.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"label": label, "exit_code": result.returncode, "source_stable": before == after, "output": str(output)}, ensure_ascii=False))
for name in ("stdout.txt", "stderr.txt"):
    content = (output / name).read_text(errors="replace")
    print(content[-16000:])
raise SystemExit(result.returncode)
