#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
GATE_DIR="$ROOT_DIR/test-support/emulator-proxy-gate"
ANDROID_DIR="$ROOT_DIR/android-companion"
TARGET_PACKAGE="com.interceptproxy.vpn.targetprobe"
TARGET_ACTIVITY="com.interceptproxy.vpn.isolationprobe.ProbeActivity"
TARGET_APK="$ANDROID_DIR/target-probe/build/outputs/apk/debug/target-probe-debug.apk"
DLL_DEVICE_PORT=6555
TRANSACTION_DEVICE_PORT=6556
HOST_PORT="${EMULATOR_PROXY_GATE_HOST_PORT:-16555}"
TRANSACTION_HOST_PORT=$((HOST_PORT + 1))
SERIAL="${ANDROID_SERIAL:-$(adb devices | awk 'NR > 1 && $2 == "device" { print $1; exit }')}"
ISOLATED_ADB_PORT=""

if [[ -z "$SERIAL" ]]; then
  echo "No ready Android emulator/device was found" >&2
  exit 1
fi
if [[ "$(adb -s "$SERIAL" shell getprop ro.kernel.qemu | tr -d '\r')" != "1" ]]; then
  echo "Emulator proxy gate refuses non-emulator device: $SERIAL" >&2
  exit 1
fi

# platform-tools 37 在同一个 ADB server 同时挂载多台设备时，`adb -s ... forward`
# 仍可能错误返回 “more than one device/emulator”。门禁只允许 TCP 模拟器，因此为它
# 建立独立的临时 ADB server；正式应用仍使用用户选择的系统 ADB 与设备序列号。
DEVICE_COUNT="$(adb devices | awk 'NR > 1 && $2 == "device" { count++ } END { print count + 0 }')"
if [[ "$DEVICE_COUNT" -gt 1 ]]; then
  ISOLATED_ADB_PORT="$(ruby -rsocket -e 's=TCPServer.new("127.0.0.1",0); puts s.local_address.ip_port; s.close')"
  adb -P "$ISOLATED_ADB_PORT" start-server >/dev/null
  adb -P "$ISOLATED_ADB_PORT" connect "$SERIAL" >/dev/null
  export ADB_SERVER_SOCKET="tcp:127.0.0.1:$ISOLATED_ADB_PORT"
fi

RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/emulator-proxy-gate.XXXXXX")"
READY_FILE="$RUN_DIR/ready"
REPORT_DIR="$GATE_DIR/reports"
REPORT_FILE="$REPORT_DIR/latest.json"
VPN_REPORT_FILE="$RUN_DIR/vpn-joint.json"
RUST_LOG_FILE="$RUN_DIR/rust.log"
RUNNER_PID=""

cleanup() {
  adb -s "$SERIAL" reverse --remove "tcp:$DLL_DEVICE_PORT" >/dev/null 2>&1 || true
  adb -s "$SERIAL" reverse --remove "tcp:$TRANSACTION_DEVICE_PORT" >/dev/null 2>&1 || true
  if [[ -n "$RUNNER_PID" ]] && kill -0 "$RUNNER_PID" >/dev/null 2>&1; then
    kill "$RUNNER_PID" >/dev/null 2>&1 || true
    wait "$RUNNER_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "$ISOLATED_ADB_PORT" ]]; then
    env -u ADB_SERVER_SOCKET adb -P "$ISOLATED_ADB_PORT" kill-server >/dev/null 2>&1 || true
  fi
  rm -rf "$RUN_DIR"
}
trap cleanup EXIT

# 避免失败运行后误读上一次的成功报告。只有本次 Rust runner 完整通过才会重新生成。
rm -f "$REPORT_FILE"

cargo build --manifest-path "$GATE_DIR/Cargo.toml"
EMULATOR_PROXY_GATE_DATA_DIR="$RUN_DIR/data" \
EMULATOR_PROXY_GATE_READY_FILE="$READY_FILE" \
EMULATOR_PROXY_GATE_REPORT_FILE="$REPORT_FILE" \
EMULATOR_PROXY_GATE_VPN_REPORT_FILE="$VPN_REPORT_FILE" \
EMULATOR_PROXY_GATE_HOST_PORT="$HOST_PORT" \
  "$GATE_DIR/target/debug/emulator-proxy-gate" >"$RUST_LOG_FILE" 2>&1 &
RUNNER_PID=$!

for _ in $(seq 1 300); do
  if [[ -f "$READY_FILE" ]]; then
    break
  fi
  if ! kill -0 "$RUNNER_PID" >/dev/null 2>&1; then
    cat "$RUST_LOG_FILE" >&2
    exit 1
  fi
  sleep 0.1
done
if [[ ! -f "$READY_FILE" ]]; then
  echo "Rust gate did not become ready" >&2
  cat "$RUST_LOG_FILE" >&2
  exit 1
fi

# Rust runner 在临时 Workspace 中生成 Listener UUID。Android Profile 只保存
# 稳定 Listener 引用，因此联合门禁从本轮 ready 文件读取真实 ID，
# 不得使用手写假 ID 绕过 Workspace 引用校验。
DLL_LISTENER_ID="$(sed -n 's/^dll_listener_id=//p' "$READY_FILE")"
TRANSACTION_LISTENER_ID="$(sed -n 's/^transaction_listener_id=//p' "$READY_FILE")"
if [[ -z "$DLL_LISTENER_ID" || -z "$TRANSACTION_LISTENER_ID" ]]; then
  echo "Rust gate ready file did not expose listener IDs" >&2
  cat "$READY_FILE" >&2
  exit 1
fi

adb -s "$SERIAL" reverse "tcp:$DLL_DEVICE_PORT" "tcp:$HOST_PORT"
adb -s "$SERIAL" reverse "tcp:$TRANSACTION_DEVICE_PORT" "tcp:$TRANSACTION_HOST_PORT"
ANDROID_SERIAL="$SERIAL" gradle -p "$ANDROID_DIR" \
  :app:connectedDebugAndroidTest \
  :target-probe:assembleDebug \
  -Pandroid.testInstrumentationRunnerArguments.class=com.interceptproxy.vpn.EmulatorProxyGateTest \
  -Pandroid.testInstrumentationRunnerArguments.interceptProxyGateEnabled=true

# connectedDebugAndroidTest 会卸载被测 APK。重新安装 Companion 后，用 shell UID 作为
# Payment 协议等价客户端，让两个代理入口在定向 VPN 延迟下各完成一次 D48 往返。
adb -s "$SERIAL" install -r "$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk" >/dev/null
adb -s "$SERIAL" install -r "$TARGET_APK" >/dev/null
# Instrumentation 可能在同一个 Companion 进程里留下尚在收尾的原生会话。联合门禁从
# 一个明确的新进程开始，避免上一测试周期的异步 SOCKS 任务污染本轮 last_error。
adb -s "$SERIAL" shell am force-stop com.interceptproxy.vpn
adb -s "$SERIAL" shell am start -n com.interceptproxy.vpn/.AdbControlActivity \
  --es command wake_control_server >/dev/null
adb -s "$SERIAL" shell appops set com.interceptproxy.vpn ACTIVATE_VPN allow
TARGET_UID="$(adb -s "$SERIAL" shell cmd package list packages -U "$TARGET_PACKAGE" \
  | tr -d '\r' | sed -n 's/.*uid:\([0-9][0-9]*\).*/\1/p')"
if [[ -z "$TARGET_UID" ]]; then
  echo "Could not resolve target probe UID" >&2
  exit 1
fi
set +e
ANDROID_SERIAL="$SERIAL" \
EMULATOR_PROXY_GATE_HOST_PORT="$HOST_PORT" \
EMULATOR_PROXY_GATE_VPN_REPORT_FILE="$VPN_REPORT_FILE" \
EMULATOR_PROXY_GATE_TARGET_PACKAGE="$TARGET_PACKAGE" \
EMULATOR_PROXY_GATE_TARGET_ACTIVITY="$TARGET_ACTIVITY" \
EMULATOR_PROXY_GATE_TARGET_UID="$TARGET_UID" \
EMULATOR_PROXY_GATE_DLL_LISTENER_ID="$DLL_LISTENER_ID" \
EMULATOR_PROXY_GATE_TRANSACTION_LISTENER_ID="$TRANSACTION_LISTENER_ID" \
  ruby "$GATE_DIR/vpn_joint_probe.rb"
VPN_PROBE_STATUS=$?
set -e
if [[ "$VPN_PROBE_STATUS" -ne 0 ]]; then
  cat "$RUST_LOG_FILE" >&2
  exit "$VPN_PROBE_STATUS"
fi

set +e
wait "$RUNNER_PID"
RUNNER_STATUS=$?
set -e
RUNNER_PID=""
cat "$RUST_LOG_FILE"
if [[ "$RUNNER_STATUS" -ne 0 ]]; then
  exit "$RUNNER_STATUS"
fi
echo "Android emulator proxy gate report: $REPORT_FILE"
