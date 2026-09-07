from pathlib import Path
from datetime import datetime
from zoneinfo import ZoneInfo
import hashlib,json
base=Path("docs/testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts")
integrated=base/"integrated"
backend=json.loads((integrated/"backend-snapshot-verification.json").read_text())["resources"]
fix=json.loads((base/"backend-review-fix/metadata.json").read_text())["source_snapshots"]
overrides={r["repository_path"]:r for r in fix}
rows=[]
for r in backend:
 source=r["source"]; archive=Path(r["archive"]); expected=r["sha256"]
 if source in overrides:
  replacement=overrides[source]
  before=base/"backend-review-fix"/replacement["before_snapshot"]
  assert before.read_bytes()==archive.read_bytes(),source
  assert replacement["before_sha256"]==expected,source
  archive=base/"backend-review-fix"/replacement["after_snapshot"]
  expected=replacement["after_sha256"]
 rows.append({"source":source,"archive":str(archive),"sha256":expected})
rows+=json.loads((integrated/"frontend-snapshot-verification.json").read_text())["resources"]
for r in json.loads((integrated/"integration-resources.json").read_text())["resources"]:
 rows.append({"source":r["original_path"],"archive":str(integrated/r["archive"]),"sha256":r["sha256"]})
for r in rows:
 archived=Path(r["archive"]).read_bytes()
 assert hashlib.sha256(archived).hexdigest()==r["sha256"],r["archive"]
 assert Path(r["source"]).read_bytes()==archived,r["source"]
print(json.dumps({"result":"PASS","checked_at":datetime.now(ZoneInfo("Asia/Shanghai")).isoformat(),"method":"Original B1 inventory with exactly two explicit review-fix replacements; F1 v4-final and integration configuration/document snapshots. Every digest and current source is compared. Original 969-test state is not relabeled as the post-fix state.","resource_count":len(rows),"resources":rows},indent=2))
