from pathlib import Path
import subprocess, tempfile, shutil, zipfile, struct
p = Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory(prefix="quick-workflow-replay-") as tmp:
    dst = Path(tmp)
    shutil.copytree(p / "resources/green", dst, dirs_exist_ok=True)
    subprocess.run(["deno", "test", "--no-config", "-A", str(dst / "scripts/windows-build-only-workflow.test.mjs")], check=True, cwd=dst)
a = p / "resources/artifact"
assert sorted(str(f.relative_to(a)) for f in a.rglob("*") if f.is_file()) == ["intercept-proxy.exe", "resources/android-companion.apk"]
with (a / "intercept-proxy.exe").open("rb") as f:
    assert f.read(2) == b"MZ"
    f.seek(60)
    offset = struct.unpack("<I", f.read(4))[0]
    f.seek(offset)
    assert f.read(4) == b"PE\0\0"
    assert struct.unpack("<H", f.read(2))[0] == 0x8664
    f.seek(offset + 24)
    assert struct.unpack("<H", f.read(2))[0] == 0x20b
with zipfile.ZipFile(a / "resources/android-companion.apk") as z:
    assert z.testzip() is None
    assert "AndroidManifest.xml" in z.namelist()
print("PASS: archived workflow contract and executable/APK layout/integrity")
