from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

case = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="universal-evidence-replay-") as name:
    work = Path(name)
    (work / "macos").mkdir()
    (work / "dmg").mkdir()
    subprocess.run(["ditto", "-x", "-k", str(case / "outputs/artifacts/Intercept Proxy.app.zip"), str(work / "macos")], check=True)
    shutil.copy2(case / "outputs/artifacts/Intercept Proxy_1.0.0_universal.dmg", work / "dmg")
    with tarfile.open(case / "resources/build-inputs.tar.gz") as archive:
        source = archive.extractfile("scripts/verify-macos-universal.mjs")
        if source is None:
            raise RuntimeError("Archived verifier missing")
        verifier = work / "verify-macos-universal.mjs"
        verifier.write_bytes(source.read())
    subprocess.run(["deno", "run", "-A", str(verifier), str(work)], check=True)
print("PASS: archived App and mounted DMG architecture, identifier, strict signature; detached and temporary files removed")
