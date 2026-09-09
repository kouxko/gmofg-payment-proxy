#!/usr/bin/env python3
import argparse, hashlib, json, pathlib, re, subprocess, tomllib
p = argparse.ArgumentParser()
p.add_argument("--repo", required=True)
a = p.parse_args()
r = pathlib.Path(a.repo).resolve()
expected = "1.0.1"
assert json.loads((r/"package.json").read_text())["version"] == expected
assert json.loads((r/"src-tauri/tauri.conf.json").read_text())["version"] == expected
assert tomllib.loads((r/"src-tauri/Cargo.toml").read_text())["workspace"]["package"]["version"] == expected
android = (r/"android-companion/app/build.gradle.kts").read_text()
assert re.search(r'versionCode\s*=\s*101\b', android)
assert re.search(r'versionName\s*=\s*"1\.0\.1"', android)
assert '"Intercept Proxy_1.0.1_universal.dmg"' in (r/"scripts/build-macos-universal.mjs").read_text()
cmd = ["cargo", "metadata", "--manifest-path", "src-tauri/Cargo.toml", "--format-version", "1", "--locked", "--offline", "--no-deps"]
m = json.loads(subprocess.check_output(cmd, cwd=r, text=True))
members = set(m["workspace_members"])
packages = {x["name"]: x["version"] for x in m["packages"] if x["id"] in members}
assert len(packages) == 13 and all(v == expected for v in packages.values())
baseline = pathlib.Path(__file__).resolve().parents[1]/"resources/baseline-Cargo.lock"
before = tomllib.loads(baseline.read_text())["package"]
after = tomllib.loads((r/"src-tauri/Cargo.lock").read_text())["package"]
assert len(before) == len(after)
changed = []
for old, new in zip(before, after):
    if old == new:
        continue
    x, y = old.copy(), new.copy()
    assert x.pop("version") == "1.0.0" and y.pop("version") == expected
    assert x == y and "source" not in x and x["name"] in packages
    changed.append(x["name"])
assert set(changed) == set(packages)
files = ["package.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "android-companion/app/build.gradle.kts", "scripts/build-macos-universal.mjs"]
print(json.dumps({"result":"PASS", "app_version":expected, "android_version_code":101, "proxy_version":expected, "workspace_packages":packages, "external_dependencies_unchanged":True, "dmg_filename":"Intercept Proxy_1.0.1_universal.dmg", "source_sha256":{f:hashlib.sha256((r/f).read_bytes()).hexdigest() for f in files}}, indent=2))
