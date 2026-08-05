#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
ANDROID_SERIAL="${ANDROID_SERIAL:-127.0.0.1:6555}"

# `connectedDebugAndroidTest` 会在测试结束后卸载被测 APK。弱网门禁必须能够独立运行，
# 不能依赖开发者之前是否手工安装过 Companion，因此每次先构建并覆盖安装同一 debug 包。
# `-r` 保留已经授予的 VpnService 用户授权，避免把网络语义回归误判成授权流程失败。
(
  cd "$REPO_DIR/android-companion"
  gradle --no-daemon :app:assembleDebug :isolation-probe:assembleDebug :target-probe:assembleDebug
)
adb -s "$ANDROID_SERIAL" install -r "$REPO_DIR/android-companion/app/build/outputs/apk/debug/app-debug.apk" >/dev/null
adb -s "$ANDROID_SERIAL" install -r "$REPO_DIR/android-companion/isolation-probe/build/outputs/apk/debug/isolation-probe-debug.apk" >/dev/null
TARGET_APK="$REPO_DIR/android-companion/target-probe/build/outputs/apk/debug/target-probe-debug.apk"
adb -s "$ANDROID_SERIAL" install -r "$TARGET_APK" >/dev/null

# 这是仅用于自动化门禁的模拟器脚本。全新安装会清除 Android 保存的 VPN 同意状态，
# 因而通过 shell app-op 恢复测试授权；正式桌面流程仍必须由用户在系统授权页确认。
adb -s "$ANDROID_SERIAL" shell appops set com.interceptproxy.vpn ACTIVATE_VPN allow

TARGET_UID="$(adb -s "$ANDROID_SERIAL" shell cmd package list packages -U com.interceptproxy.vpn.targetprobe \
  | tr -d '\r' | sed -n 's/.*uid:\([0-9][0-9]*\).*/\1/p')"
if [[ -z "$TARGET_UID" ]]; then
  echo "Could not resolve target probe UID" >&2
  exit 1
fi

ANDROID_VPN_GATE_TARGET_UID="$TARGET_UID" \
  exec ruby "$SCRIPT_DIR/run.rb"
