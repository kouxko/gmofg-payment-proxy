"""Verify the archived source and replay a local case without changing evidence."""
import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from zoneinfo import ZoneInfo

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("repository", type=Path)
parser.add_argument("case", choices=["android-active-profile", "rules-save-ownership", "exchange-error-observation", "socket-resource-limits", "integrated-verification"])
parser.add_argument("--verify-only", action="store_true")
args = parser.parse_args()
evidence = Path(__file__).resolve().parents[2]
repository = args.repository.resolve()
inventory = json.loads((evidence / "integrated-verification/resources/source-inventory.json").read_text())
differences = []
for relative, expected in inventory["files"].items():
    path = repository / relative
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected["sha256"]:
        differences.append(relative)
if differences:
    raise SystemExit("Source differs from the tested snapshot:\n" + "\n".join(differences))
print(f"Verified {len(inventory['files'])} archived source and contract files.", flush=True)
if args.verify_only:
    raise SystemExit(0)

case = json.loads((evidence / args.case / "metadata.json").read_text())
output = Path(tempfile.mkdtemp(prefix="proxy-review-replay-"))
print(f"New replay output: {output}", flush=True)
for index, command in enumerate(case["commands"], 1):
    started = datetime.now(ZoneInfo("Asia/Shanghai")).isoformat()
    with (output / f"{index}.log").open("w") as log:
        result = subprocess.run(command, cwd=repository, stdout=log, stderr=subprocess.STDOUT)
    (output / f"{index}.json").write_text(json.dumps({"command": command, "cwd": str(repository), "started_at": started, "completed_at": datetime.now(ZoneInfo("Asia/Shanghai")).isoformat(), "exit_code": result.returncode}, indent=2) + "\n")
    print(f"{index}: exit {result.returncode}: {' '.join(command)}", flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
