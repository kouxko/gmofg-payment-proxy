#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER_MANIFEST="$SCRIPT_DIR/Cargo.toml"
RUNNER_BIN="$SCRIPT_DIR/target/debug/gmofg-headless-device-runner"

: "${GMOFG_PROXY_CA_DER:?set GMOFG_PROXY_CA_DER to the existing proxy CA DER path}"
: "${GMOFG_CLIENT_P12:?set GMOFG_CLIENT_P12 to the existing client PKCS12 path}"
: "${GMOFG_CREDIT_TID:?set GMOFG_CREDIT_TID in the invoking environment}"
: "${GMOFG_CONFIRM_CODE:?set GMOFG_CONFIRM_CODE in the invoking environment}"
: "${GMOFG_PAYMENT_PASSWORD:?set GMOFG_PAYMENT_PASSWORD in the invoking environment}"

GMOFG_DEVICE_SERIAL="${GMOFG_DEVICE_SERIAL:-2740072778}"
GMOFG_PROXY_URL="${GMOFG_PROXY_URL:-https://10.0.34.50:16127/}"
GMOFG_APP_DATA_DIR="${GMOFG_APP_DATA_DIR:-$HOME/Library/Application Support/com.gmofg.paymentproxy}"
EVIDENCE_ROOT="${GMOFG_EVIDENCE_ROOT:-$(mktemp -d /private/tmp/gmofg-headless-scenarios.XXXXXX)}"

for required_command in adb awk base64 cargo rg tr; do
  if ! command -v "$required_command" >/dev/null 2>&1; then
    echo "required command is unavailable: $required_command" >&2
    exit 1
  fi
done

mkdir -p "$EVIDENCE_ROOT"
cargo build --manifest-path "$RUNNER_MANIFEST"

PROXY_CA_B64="$(base64 <"$GMOFG_PROXY_CA_DER" | tr -d '\r\n')"
CLIENT_P12_B64="$(base64 <"$GMOFG_CLIENT_P12" | tr -d '\r\n')"
RUNNER_PID=""
LAST_TEST_SECONDS=""

cleanup_runner() {
  if [[ -n "$RUNNER_PID" ]] && kill -0 "$RUNNER_PID" 2>/dev/null; then
    kill -INT "$RUNNER_PID" 2>/dev/null || true
    wait "$RUNNER_PID" 2>/dev/null || true
  fi
}
trap cleanup_runner EXIT INT TERM

wait_for_log() {
  local pid="$1"
  local log_file="$2"
  local pattern="$3"
  local attempts="$4"
  local index
  for ((index = 0; index < attempts; index += 1)); do
    if rg -q "$pattern" "$log_file" 2>/dev/null; then
      return 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      tail -n 80 "$log_file" >&2 || true
      return 1
    fi
    sleep 0.1
  done
  return 1
}

run_scenario() {
  local scenario="$1"
  local evidence_name="$2"
  local scenario_dir="$EVIDENCE_ROOT/$evidence_name"
  local runner_log="$scenario_dir/runner.txt"
  local instrumentation_log="$scenario_dir/instrumentation.txt"
  local device_log="$scenario_dir/device-logcat.txt"
  local adb_status

  mkdir -p "$scenario_dir"
  GMOFG_APP_DATA_DIR="$GMOFG_APP_DATA_DIR" \
    "$RUNNER_BIN" "$scenario" >"$runner_log" 2>&1 &
  RUNNER_PID="$!"
  wait_for_log "$RUNNER_PID" "$runner_log" \
    "HEADLESS_READY scenario=$scenario" 600

  adb -s "$GMOFG_DEVICE_SERIAL" logcat -c >/dev/null 2>&1 || true
  set +e
  adb -s "$GMOFG_DEVICE_SERIAL" shell am instrument -w -r \
    -e class jp.gmofg.payment.proxy.DllProxyRealDeviceTest#creditDllReturnsD48ThroughProxy \
    -e proxyUrl "$GMOFG_PROXY_URL" \
    -e proxyCaBase64 "$PROXY_CA_B64" \
    -e clientP12Base64 "$CLIENT_P12_B64" \
    -e creditTid "$GMOFG_CREDIT_TID" \
    -e confirmCode "$GMOFG_CONFIRM_CODE" \
    -e password "$GMOFG_PAYMENT_PASSWORD" \
    jp.gmofg.payment.test/androidx.test.runner.AndroidJUnitRunner \
    >"$instrumentation_log" 2>&1
  adb_status="$?"
  set -e

  wait_for_log "$RUNNER_PID" "$runner_log" \
    "HEADLESS_CLEAN scenario=$scenario remaining_test_rules=0" 1300
  wait "$RUNNER_PID"
  RUNNER_PID=""

  adb -s "$GMOFG_DEVICE_SERIAL" logcat -d -v threadtime \
    | rg 'DllProxyRealDeviceTest|DLL_PROXY_D48_CONFIRMED' \
    >"$device_log" || true

  rg -q '"tls_summary":"TLS 1.2 mTLS' "$runner_log"
  if [[ "$scenario" == "baseline" || "$scenario" == "delay" ]]; then
    rg -q 'OK \(1 test\)' "$instrumentation_log"
    rg -q 'DLL_PROXY_D48_CONFIRMED.*errorCode=D48' "$device_log"
  elif [[ "$scenario" == "custom-status" ]]; then
    rg -q 'proxy returned HTTP 503' "$instrumentation_log"
    rg -q 'FAILURES!!!' "$instrumentation_log"
  elif [[ "$scenario" == "invalid-json" ]]; then
    rg -q 'server response is not a CreditDLL message; bodyBytes=8' "$instrumentation_log"
    rg -q 'FAILURES!!!' "$instrumentation_log"
  fi
  if [[ "$scenario" != "baseline" ]]; then
    rg -q '"rule_hit_count":1' "$runner_log"
    rg -q '\[命中\] 全部匹配条件满足' "$runner_log"
  fi

  LAST_TEST_SECONDS="$(awk '/^Time: / {print $2; exit}' "$instrumentation_log")"
  if [[ -z "$LAST_TEST_SECONDS" ]]; then
    echo "missing Android instrumentation time for $evidence_name" >&2
    return 1
  fi
  echo "SCENARIO_RESULT name=$evidence_name scenario=$scenario android_test_seconds=$LAST_TEST_SECONDS adb_status=$adb_status"
}

run_scenario baseline baseline-initial
run_scenario custom-status custom-status
run_scenario invalid-json invalid-json
run_scenario baseline baseline-before-delay
BASELINE_BEFORE_DELAY_SECONDS="$LAST_TEST_SECONDS"
run_scenario delay delay-10000ms
DELAY_SECONDS="$LAST_TEST_SECONDS"

DELAY_DELTA_SECONDS="$(
  awk -v delayed="$DELAY_SECONDS" -v baseline="$BASELINE_BEFORE_DELAY_SECONDS" \
    'BEGIN { printf "%.3f", delayed - baseline }'
)"
awk -v delta="$DELAY_DELTA_SECONDS" \
  'BEGIN { if (delta < 8.5) exit 1 }'
echo "DELAY_ASSERT baseline_seconds=$BASELINE_BEFORE_DELAY_SECONDS delay_seconds=$DELAY_SECONDS delta_seconds=$DELAY_DELTA_SECONDS minimum_seconds=8.5"

run_scenario baseline baseline-final
echo "HEADLESS_DEVICE_SCENARIOS_OK evidence_root=$EVIDENCE_ROOT"
