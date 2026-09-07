#!/usr/bin/env bash
set -euo pipefail

# 桌面安装包只能携带已经通过门禁的 release Companion。这个脚本固定执行顺序：
# Rust/JVM 测试与 release APK -> 签名/ABI/对齐校验 -> Tauri 资源暂存。
repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
companion_root="$repository_root/android-companion"
apk="$companion_root/app/build/outputs/apk/release/app-release.apk"
fingerprint_file="$companion_root/signing/certificate-sha256.txt"

[[ -f "$fingerprint_file" ]] || {
  echo "缺少固定签名证书指纹：$fingerprint_file" >&2
  exit 1
}
expected_fingerprint="$(tr -d ':[:space:]' < "$fingerprint_file")"

(
  cd "$companion_root"
  gradle --no-daemon \
    :app:lintRelease \
    :app:testDebugUnitTest \
    :app:assembleRelease \
    :isolation-probe:assembleDebug \
    :target-probe:assembleDebug
)

"$repository_root/scripts/verify-android-companion.sh" \
  "$apk" \
  --release \
  --expected-cert-sha256 "$expected_fingerprint"
"$repository_root/scripts/stage-android-companion.sh" "$apk"

echo "Companion release APK 已构建、校验并放入桌面资源：$apk"
