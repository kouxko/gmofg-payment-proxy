#!/usr/bin/env python3
"""Replay against current source; always write new output rather than overwrite archived evidence."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

root = next(p for p in Path(__file__).resolve().parents if (p / "src-tauri/Cargo.toml").exists())
output = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp") / ("body-charset-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S"))
output.mkdir(parents=True, exist_ok=False)
os.chdir(root)

def state():
    changed = subprocess.check_output(["git", "diff", "--name-only", "HEAD", "--", "src", "src-tauri"], text=True).splitlines()
    untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard", "src", "src-tauri"], text=True).splitlines()
    archived = json.loads((Path(__file__).resolve().parent.parent / "resources/source-files.json").read_text())
    return {name: hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in sorted(set(archived + changed + untracked)) if Path(name).is_file()}

manifest = ["--manifest-path", "src-tauri/Cargo.toml"]
checks = [
    ("fmt", ["cargo", "fmt", *manifest, "--all", "--", "--check"]),
    ("exchange", ["cargo", "test", *manifest, "-p", "intercept-proxy-exchange", "--lib", "--", "--nocapture"]),
    ("codec", ["cargo", "test", *manifest, "-p", "intercept-proxy-infrastructure", "--lib", "body_codecs", "--", "--nocapture"]),
    ("mock", ["cargo", "test", *manifest, "-p", "intercept-proxy-application", "--lib", "exchange_mock", "--", "--nocapture"]),
    ("consumer", ["cargo", "test", *manifest, "-p", "intercept-proxy", "--lib", "runtime_logs::exchange_ui_layer", "--", "--nocapture"]),
    ("clippy", ["cargo", "clippy", *manifest, "-p", "intercept-proxy-exchange", "-p", "intercept-proxy-application", "-p", "intercept-proxy-infrastructure", "-p", "intercept-proxy", "--all-targets", "--", "-D", "warnings"]),
    ("frontend", ["node_modules/.bin/vitest", "run", "src/features/capture/exchange-observation-detail.test.tsx"]),
    ("eslint", ["node_modules/.bin/eslint", "src/features/capture/exchange-observation-detail.test.tsx"]),
    ("typecheck", ["node_modules/.bin/tsc", "--noEmit"]),
    ("architecture", ["deno", "run", "-A", "scripts/check-architecture-boundaries.mjs"]),
    ("architecture-docs", ["deno", "run", "-A", "scripts/check-architecture-docs.mjs"]),
    ("diff", ["git", "diff", "--check"]),
]
if len(sys.argv) > 2:
    selected = set(sys.argv[2:])
    assert selected <= {name for name, _ in checks}, "unknown check"
    checks = [(name, command) for name, command in checks if name in selected]

initial = state()
results = []
for name, command in checks:
    started = datetime.datetime.now().astimezone().isoformat()
    print("RUN", name, flush=True)
    with (output / (name + ".log")).open("w") as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    stable = state() == initial
    results.append(dict(name=name, command=command, started_at=started, ended_at=datetime.datetime.now().astimezone().isoformat(), exit_code=result.returncode, source_stable=stable))
    (output / "results.json").write_text(json.dumps(dict(source_sha256=initial, results=results), ensure_ascii=False, indent=2) + "\n")
    print(name, result.returncode, "stable=" + str(stable), flush=True)
    if result.returncode or not stable:
        raise SystemExit(1)
print("PASS", output, flush=True)
