#!/usr/bin/env python3
"""Run the affected backend and architecture checks from the repository root."""
from pathlib import Path
from datetime import datetime
from zoneinfo import ZoneInfo
import subprocess, json, sys
root = Path.cwd()
base = Path(__file__).resolve().parents[1]
post_review = "--post-review" in sys.argv
prefix = "post-review-" if post_review else ""
def source_state():
 paths=set()
 for args in (["git","diff","--name-only","--","src-tauri","scripts","docs/architecture"],
              ["git","ls-files","--others","--exclude-standard","--","src-tauri","scripts"]):
  paths.update(subprocess.check_output(args,cwd=root,text=True).splitlines())
 result={}
 for name in sorted(paths):
  path=root/name
  result[name]=({"size":path.stat().st_size,"mtime_ns":path.stat().st_mtime_ns}
                if path.is_file() else {"deleted":True})
 return result
before=source_state()
(base/"outputs"/f"{prefix}source-state-before.json").write_text(json.dumps(before,indent=2)+"\n")
commands = [
 ("rust-check", ["cargo", "check", "--manifest-path", "src-tauri/Cargo.toml", "--workspace", "--all-targets", "--all-features"]),
 ("write-commit-proof", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-runtime", "--lib", "write_commit", "--", "--nocapture"]),
 ("rust-tests", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--workspace", "--all-targets", "--all-features", "--no-fail-fast"]),
 ("rust-clippy", ["cargo", "clippy", "--manifest-path", "src-tauri/Cargo.toml", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]),
 ("rust-fmt", ["cargo", "fmt", "--manifest-path", "src-tauri/Cargo.toml", "--all", "--", "--check"]),
 ("architecture", ["deno", "task", "scan:architecture"]),
 ("pipeline-guards", ["deno", "test", "-A", "scripts/check-task-20260829-002-phase10-http-pipeline.test.mjs", "scripts/check-task-20260829-002-phase11-socket-pipeline.test.mjs"]),
 ("source-size", ["deno", "run", "-A", "scripts/check-source-file-sizes.mjs"]),
 ("script-lint", ["deno", "task", "lint"]),
 ("diff-check", ["git", "diff", "--check"]),
]
if post_review:
 commands = [
  ("frame-rejection-green", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure", "--lib", "external_relay::capabilities", "--", "--nocapture"]),
  ("infrastructure-tests", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-infrastructure", "--lib", "--all-features"]),
  ("runtime-tests", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "-p", "intercept-proxy-runtime", "--lib", "--all-features"]),
 ] + [(name,command) for name,command in commands if name in {"rust-clippy","rust-fmt","architecture","pipeline-guards","diff-check"}]
results=[]
for name, command in commands:
 start=datetime.now(ZoneInfo("Asia/Shanghai")).isoformat()
 print(f"START {name}",flush=True)
 with (base/"outputs"/f"{prefix}{name}.log").open("w") as output:
  output.write("Command: "+" ".join(command)+"\nStarted: "+start+"\n");output.flush()
  result=subprocess.run(command,cwd=root,stdout=output,stderr=subprocess.STDOUT)
  output.write("\nExit code: "+str(result.returncode)+"\n")
 results.append({"check":name,"command":command,"started_at":start,"finished_at":datetime.now(ZoneInfo("Asia/Shanghai")).isoformat(),"exit_code":result.returncode})
 print(f"DONE {name}: {result.returncode}",flush=True)
 (base/"outputs"/f"{prefix}results.json").write_text(json.dumps(results,ensure_ascii=False,indent=2)+"\n")
after=source_state()
(base/"outputs"/f"{prefix}source-state-after.json").write_text(json.dumps(after,indent=2)+"\n")
stable=before==after
(base/"outputs"/f"{prefix}source-stability.json").write_text(json.dumps({"stable":stable,"files":len(before)},indent=2)+"\n")
sys.exit(0 if stable and all(item["exit_code"]==0 for item in results) else 1)
