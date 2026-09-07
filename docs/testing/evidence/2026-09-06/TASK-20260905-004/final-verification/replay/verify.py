#!/usr/bin/env python3
"""Run the final local verification matrix; preserve failures and source stability."""
from pathlib import Path
from datetime import datetime
from zoneinfo import ZoneInfo
import json, subprocess, sys
base=Path(__file__).resolve().parents[1]
root=Path.cwd()

def state():
 paths=set()
 for command in (["git","diff","--name-only","--","src-tauri","src","scripts","docs/architecture","docs/requirements.md","docs/user-operation-guide.md"], ["git","ls-files","--others","--exclude-standard","--","src-tauri","src","scripts"]):
  paths.update(subprocess.check_output(command,text=True).splitlines())
 return {name:({"size":Path(name).stat().st_size,"mtime_ns":Path(name).stat().st_mtime_ns} if Path(name).is_file() else {"deleted":True}) for name in sorted(paths)}

commands=[
 ("cargo-check",["cargo","check","--manifest-path","src-tauri/Cargo.toml","--workspace","--all-targets","--all-features"]),
 ("cargo-tests",["cargo","test","--manifest-path","src-tauri/Cargo.toml","--workspace","--all-targets","--all-features","--no-fail-fast"]),
 ("clippy",["cargo","clippy","--manifest-path","src-tauri/Cargo.toml","--workspace","--all-targets","--all-features","--","-D","warnings"]),
 ("rustfmt",["cargo","fmt","--manifest-path","src-tauri/Cargo.toml","--all","--","--check"]),
 ("source-size",["deno","task","scan:source-size"]),
 ("architecture",["deno","task","scan:architecture"]),
 ("pipeline-guards",["deno","test","-A","scripts/check-task-20260829-002-phase10-http-pipeline.test.mjs","scripts/check-task-20260829-002-phase11-socket-pipeline.test.mjs"]),
 ("frontend-tests",["deno","task","test"]),
 ("typecheck",["deno","task","typecheck"]),
 ("lint",["deno","task","lint"]),
 ("diff-check",["git","diff","--check"]),
]
# Replays always use a fresh directory; never overwrite archived run evidence.
post_fix = "--post-fix" in sys.argv
run_name = "post-fix" if post_fix else "replay-" + datetime.now(ZoneInfo("Asia/Shanghai")).strftime("%Y%m%d-%H%M%S")
output_dir = base / "outputs" / run_name
output_dir.mkdir(exist_ok=False)
if post_fix:
 commands = [(name, command) for name, command in commands if name != "pipeline-guards"]
 commands = [(name, ["deno", "task", "test", "src/features/android-network/android-network-view.test.tsx", "src/features/android-network/android-network-view.network-intent.test.tsx"] if name == "frontend-tests" else command) for name, command in commands]
before=state()
(output_dir/"source-before.json").write_text(json.dumps(before,indent=2)+"\n")
results=[]
for name,command in commands:
 started=datetime.now(ZoneInfo("Asia/Shanghai")).isoformat()
 print("START",name,flush=True)
 with (output_dir/f"{name}.log").open("w") as output:
  output.write("Command: "+" ".join(command)+"\nStarted: "+started+"\n");output.flush()
  result=subprocess.run(command,cwd=root,stdout=output,stderr=subprocess.STDOUT)
  output.write("\nExit code: "+str(result.returncode)+"\n")
 results.append({"name":name,"command":command,"started_at":started,"finished_at":datetime.now(ZoneInfo("Asia/Shanghai")).isoformat(),"exit_code":result.returncode})
 (output_dir/"results.json").write_text(json.dumps(results,indent=2)+"\n")
 print("DONE",name,result.returncode,flush=True)
after=state()
(output_dir/"source-after.json").write_text(json.dumps(after,indent=2)+"\n")
(output_dir/"source-stability.json").write_text(json.dumps({"stable":before==after,"files":len(before)},indent=2)+"\n")
sys.exit(0 if before==after and all(row["exit_code"]==0 for row in results) else 1)
