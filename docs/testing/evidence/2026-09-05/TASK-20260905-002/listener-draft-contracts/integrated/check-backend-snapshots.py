from pathlib import Path
import hashlib,json
base=Path("docs/testing/evidence/2026-09-05/TASK-20260905-002/listener-draft-contracts/backend/resources/after")
manifest=base/"SHA256SUMS.txt"
app="src-tauri/crates/application/src/"
origins={
 "application-facade/draft.rs":app+"facade/listeners/draft.rs",
 "application-facade/listeners.rs":app+"facade/listeners.rs",
 "application-models/listener_draft.rs":app+"models/listener_draft.rs",
 "application-models/models.rs":app+"models.rs",
 "application-models/normalization.rs":app+"models/listener_draft/normalization.rs",
 "application-models/transition.rs":app+"models/listener_draft/transition.rs",
 "application-tests/listener_draft.rs":app+"requirements_tests/listener_draft.rs",
 "application-tests/requirements_tests.rs":app+"requirements_tests.rs",
 "application-tests/validation.rs":app+"requirements_tests/listener_draft/validation.rs",
 "generated/rust-types.ts":"src/generated/rust-types.ts",
 "tauri-commands/listener.rs":"src-tauri/src/commands/listener.rs",
 "tauri-commands/mod.rs":"src-tauri/src/commands/mod.rs",
}
rows=[]
for line in manifest.read_text().splitlines():
 expected, name=line.split("  ",1); archived=Path(name)
 if archived==manifest:
  continue
 relative=str(archived.relative_to(base)); source=Path(origins[relative])
 digest=hashlib.sha256(archived.read_bytes()).hexdigest()
 rows.append({"archive":name,"source":str(source),"sha256":digest,"matches_recorded_digest":digest==expected,"matches_current_source":archived.read_bytes()==source.read_bytes()})
assert len(rows)==len(origins)
passed=all(r["matches_recorded_digest"] and r["matches_current_source"] for r in rows)
print(json.dumps({"result":"PASS" if passed else "FAIL","original_manifest_defect":"The original manifest includes its own empty-file digest. That self-entry is invalid; original file is preserved. This independent inventory checks every actual archived source, omits only the manifest self-entry, and maps exact repository origins.","resources":rows},indent=2))
raise SystemExit(0 if passed else 1)
