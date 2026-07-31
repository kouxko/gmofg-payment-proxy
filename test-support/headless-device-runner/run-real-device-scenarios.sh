#!/usr/bin/env bash
set -euo pipefail

# 真实设备无 UI 验收总控脚本。
#
# 它不启动 Tauri 窗口，而是为每个场景启动一次 Rust ApplicationHost runner，再通过
# Android instrumentation 让真实 Payment 流量经过 Proxy。Rust 侧验证规则轨迹/会话/抓包，
# Android 侧验证客户端实际看到的 HTTP、正文、异常或 D48；两边证据缺一不可。
#
# 如果只想快速确认“证书和转发链路能否通”，应使用 real-device-dll-proxy 单场景探针；
# 本脚本用于规则、断点、故障模板和弱网动作的完整场景矩阵，运行时间会更长。

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER_MANIFEST="$SCRIPT_DIR/Cargo.toml"
RUNNER_BIN="$SCRIPT_DIR/target/debug/gmofg-headless-device-runner"
MATRIX_FILE="$SCRIPT_DIR/scenarios.json"

# 环境变量分为四类：
# 1. 设备/网络：设备序列号与 Android 实际访问的 Proxy URL；
# 2. Rust 数据：桌面应用数据目录、SQLite 和测试 Root CA；
# 3. Android 工程：源码、JDK、instrumentation APK 及是否自动构建安装；
# 4. 选择/证据：批次、单个场景和证据输出目录。
# 默认值面向当前实验室环境；证书、PKCS12 等敏感路径必须显式提供或指向受控文件。
GMOFG_DEVICE_SERIAL="${GMOFG_DEVICE_SERIAL:-2740072778}"
GMOFG_PROXY_URL="${GMOFG_PROXY_URL:-https://10.0.34.50:16127/}"
GMOFG_PROXY_URL_HOST_PORT="$(
  sed -E 's#^https?://([^/]+)/?.*$#\1#' <<<"$GMOFG_PROXY_URL"
)"
GMOFG_APP_DATA_DIR="${GMOFG_APP_DATA_DIR:-$HOME/Library/Application Support/com.gmofg.paymentproxy}"
GMOFG_SQLITE_PATH="${GMOFG_SQLITE_PATH:-$GMOFG_APP_DATA_DIR/gmofg-payment-proxy.sqlite3}"
GMOFG_PROXY_CA_DER="${GMOFG_PROXY_CA_DER:-/private/tmp/gmofg-proxy-root-ca.der.cer}"
GMOFG_CLIENT_P12="${GMOFG_CLIENT_P12:?set GMOFG_CLIENT_P12 to a secure PKCS12 path}"
GMOFG_ANDROID_PROJECT="${GMOFG_ANDROID_PROJECT:-$HOME/Code/jp_gmofg_payment}"
GMOFG_ANDROID_JAVA_HOME="${GMOFG_ANDROID_JAVA_HOME:-$HOME/.sdkman/candidates/java/11.0.28-ms}"
GMOFG_ANDROID_TEST_APK="${GMOFG_ANDROID_TEST_APK:-$GMOFG_ANDROID_PROJECT/payment/build/outputs/apk/androidTest/gmofg/debug/payment-gmofg-debug-androidTest.apk}"
GMOFG_PREPARE_ANDROID="${GMOFG_PREPARE_ANDROID:-1}"
GMOFG_KEEP_ANDROID_TEST_PACKAGE="${GMOFG_KEEP_ANDROID_TEST_PACKAGE:-0}"
# 批次按风险主题拆分，便于失败后只重跑相关范围：
# A=报文修改/延迟/Mock，B=连接与响应破坏，C=请求/响应断点，D=规则状态与组合顺序，
# E=各类匹配条件，F=应被 Rust 拒绝的非法配置，G=限速/抖动/间歇/中途断开等弱网动作。
# 设为 ALL 执行全部批次；GMOFG_SCENARIO 非空时会进一步缩小到一个场景。
GMOFG_BATCH="${GMOFG_BATCH:-A}"
GMOFG_SCENARIO="${GMOFG_SCENARIO:-}"
EVIDENCE_ROOT="${GMOFG_EVIDENCE_ROOT:-$(mktemp -d /private/tmp/gmofg-headless-matrix.XXXXXX)}"
RESULTS_FILE="$EVIDENCE_ROOT/results.jsonl"
DELAY_ASSERTIONS_FILE="$EVIDENCE_ROOT/delay-assertions.jsonl"
BATCH_RECOVERY_ASSERTIONS_FILE="$EVIDENCE_ROOT/batch-recovery-assertions.jsonl"
HEADLESS_MASTER_KEY_FILE=""
DEVICE_CREDIT_TID=""
DEVICE_CONFIRM_CODE=""
DEVICE_PAYMENT_PASSWORD=""
DEVICE_SETTINGS_JSON=""
LAST_ANDROID_ELAPSED_MS=""

for required_command in adb cargo jq lsof rg shasum sqlite3; do
  if ! command -v "$required_command" >/dev/null 2>&1; then
    echo "required command is unavailable: $required_command" >&2
    exit 1
  fi
done
for required_file in "$MATRIX_FILE" "$GMOFG_PROXY_CA_DER" "$GMOFG_CLIENT_P12"; do
  if [[ ! -s "$required_file" ]]; then
    echo "required file is missing or empty: $required_file" >&2
    exit 1
  fi
done
# 在连接设备前先验证矩阵结构。这里不是重复测试业务实现，而是保证：模板数量、匹配字段、
# 非法配置、弱网场景及其期望结果没有被维护者误删或改成无法判定的宽松条件。
jq -e '
  ([.scenarios[].template_id | select(. != null)] | unique | length) == 22
  and (
    [.scenarios[] | select(.batch == "E") | .match_field | select(. != null)]
    | unique
    | sort
  ) == ["certificate_fingerprint", "json_path", "path_or_request_type", "terminal_ip"]
  and (
    [.scenarios[] | select(.batch == "E") | .match_operator | select(. != null)]
    | unique
    | sort
  ) == ["contains", "equals", "regex"]
  and ([.scenarios[] | select(.batch == "E" and .expected_failed_trace == true)] | length) >= 2
  and (
    [.scenarios[] | select(.batch == "F")]
    | length == 9
    and all(.[]; .expected_invalid_error == true)
  )
  and (
    [.scenarios[] | select(.batch == "G") | .id] | sort
  ) == [
    "disconnect-downstream-mid-body",
    "disconnect-upstream-mid-body",
    "intermittent-downstream",
    "intermittent-upstream",
    "jitter-downstream",
    "jitter-upstream",
    "throttle-downstream",
    "throttle-upstream"
  ]
  and all(
    .scenarios[] | select(.batch == "G");
    .expected_rule_hits == 1
    and (
      if .id == "disconnect-upstream-mid-body"
      then (
        .android_expected_kind == "io_failure"
        and .expected_exception == "IOException"
        and .requires_batch_recovery_d48 == true
      )
      elif .id == "disconnect-downstream-mid-body"
      then (
        .android_expected_kind == "body_read_failure"
        and .expected_exception == "ProtocolException"
        and .requires_batch_recovery_d48 == true
      )
      else (
        .android_expected_kind == "d48"
        and (.minimum_elapsed_ms | type == "number" and . > 0)
      )
      end
    )
  )
  and all(
    .scenarios[];
    .android_expected_kind as $kind
    | [
        "d48",
        "http_status",
        "body_contains",
        "header_equals",
        "any_http",
        "io_failure",
        "body_read_failure",
        "http_non_d48"
      ]
    | index($kind) != null
  )
  and all(
    .scenarios[];
    if (
      .android_expected_kind == "io_failure"
      or .android_expected_kind == "body_read_failure"
    )
    then (.expected_exception | type == "string" and length > 0)
    else true
    end
  )
  and all(.scenarios[]; .implementation == "ready")
' "$MATRIX_FILE" >/dev/null
if ! adb -s "$GMOFG_DEVICE_SERIAL" get-state 2>/dev/null | rg -qx 'device'; then
  echo "Android device is not ready: $GMOFG_DEVICE_SERIAL" >&2
  exit 1
fi

mkdir -p "$EVIDENCE_ROOT"
: >"$RESULTS_FILE"
: >"$DELAY_ASSERTIONS_FILE"
: >"$BATCH_RECOVERY_ASSERTIONS_FILE"
SELECTED_SCENARIO_COUNT="$(
  jq \
    --arg batch "$GMOFG_BATCH" \
    --arg scenario "$GMOFG_SCENARIO" \
    '[.scenarios[]
      | select(
          (.batch == $batch or $batch == "ALL")
          and .implementation == "ready"
          and ($scenario == "" or .id == $scenario)
        )]
      | length' \
    "$MATRIX_FILE"
)"
if [[ "$SELECTED_SCENARIO_COUNT" == "0" ]]; then
  echo "no ready scenarios selected for batch=$GMOFG_BATCH scenario=$GMOFG_SCENARIO" >&2
  exit 1
fi

if [[ "$GMOFG_PREPARE_ANDROID" == "1" ]]; then
  # instrumentation APK 是测试包，不是桌面 UI。它在真机进程内调用 Payment 的 DLL 请求路径，
  # 因而能够证明 Android 客户端实际观察结果，而不仅是主机侧模拟一个 HTTP 请求。
  (
    export JAVA_HOME="$GMOFG_ANDROID_JAVA_HOME"
    export PATH="$JAVA_HOME/bin:$PATH"
    cd "$GMOFG_ANDROID_PROJECT"
    bash ./gradlew :payment:assembleGmofgDebugAndroidTest --no-daemon
  ) >"$EVIDENCE_ROOT/android-build.txt" 2>&1
  if [[ ! -s "$GMOFG_ANDROID_TEST_APK" ]]; then
    echo "Android instrumentation APK was not produced: $GMOFG_ANDROID_TEST_APK" >&2
    exit 1
  fi
  adb -s "$GMOFG_DEVICE_SERIAL" install -r -t "$GMOFG_ANDROID_TEST_APK" \
    >"$EVIDENCE_ROOT/android-install.txt" 2>&1
fi

cargo build --manifest-path "$RUNNER_MANIFEST" >"$EVIDENCE_ROOT/rust-build.txt" 2>&1
RUNNER_PID=""
INSTRUMENTATION_PID=""
SOURCE_DIGEST="$(
  # 证据目录记录源码状态、runner 和 APK 摘要。这样以后查看日志时，可以确认它们对应哪一份
  # 已提交/未提交代码；摘要用于可追溯性，不代表二进制签名或发布可信度。
  {
    git -C "$SCRIPT_DIR/../.." rev-parse HEAD 2>/dev/null || true
    git -C "$SCRIPT_DIR/../.." status --porcelain 2>/dev/null || true
    git -C "$SCRIPT_DIR/../.." diff --binary HEAD -- src-tauri test-support \
      2>/dev/null || true
    git -C "$SCRIPT_DIR/../.." \
      ls-files --others --exclude-standard -z -- src-tauri test-support \
      | while IFS= read -r -d '' file; do
          shasum -a 256 "$SCRIPT_DIR/../../$file"
        done
    find "$SCRIPT_DIR" -maxdepth 2 -type f ! -path '*/target/*' -print0 \
      | sort -z \
      | xargs -0 shasum -a 256
  } | shasum -a 256 | awk '{print $1}'
)"
RUNNER_SHA256="$(shasum -a 256 "$RUNNER_BIN" | awk '{print $1}')"
ANDROID_APK_SHA256="$(
  if [[ -s "$GMOFG_ANDROID_TEST_APK" ]]; then
    shasum -a 256 "$GMOFG_ANDROID_TEST_APK" | awk '{print $1}'
  else
    echo "unavailable"
  fi
)"

read_device_setting() {
  local key="$1"
  local output
  output="$(
    adb -s "$GMOFG_DEVICE_SERIAL" shell content query \
      --uri "content://jp.gmofg.app.provider.settings/$key"
  )"
  sed -n 's/^Row: 0 [^=]*=//p' <<<"$output" | tail -n 1
}

cleanup_device_artifacts() {
  # 设备上的证书、PKCS12 与 Payment 设置只为本轮 instrumentation 准备。
  # 无论成功、失败还是 Ctrl-C，都应删除，避免敏感材料残留在测试包私有目录。
  DEVICE_CREDIT_TID=""
  DEVICE_CONFIRM_CODE=""
  DEVICE_PAYMENT_PASSWORD=""
  DEVICE_SETTINGS_JSON=""
  unset \
    DEVICE_CREDIT_TID \
    DEVICE_CONFIRM_CODE \
    DEVICE_PAYMENT_PASSWORD \
    DEVICE_SETTINGS_JSON
  adb -s "$GMOFG_DEVICE_SERIAL" exec-out \
    run-as jp.gmofg.payment.test sh -c \
    'rm -f files/gmofg-proxy-ca.der files/gmofg-client.p12 files/gmofg-device-settings.json' \
    </dev/null >/dev/null 2>&1 || true
  adb -s "$GMOFG_DEVICE_SERIAL" logcat -c >/dev/null 2>&1 || true
  if [[ "$GMOFG_KEEP_ANDROID_TEST_PACKAGE" != "1" ]]; then
    adb -s "$GMOFG_DEVICE_SERIAL" uninstall jp.gmofg.payment.test \
      >/dev/null 2>&1 || true
  fi
}

cleanup_all() {
  # 清理顺序很重要：先停止仍在发请求的 instrumentation，再停止 Rust runner，
  # 然后删除主密钥临时文件和设备材料，避免进程继续读取已删除或半清理的数据。
  if [[ -n "$INSTRUMENTATION_PID" ]] && kill -0 "$INSTRUMENTATION_PID" 2>/dev/null; then
    adb -s "$GMOFG_DEVICE_SERIAL" shell am force-stop jp.gmofg.payment.test \
      >/dev/null 2>&1 || true
    kill -TERM "$INSTRUMENTATION_PID" 2>/dev/null || true
    wait "$INSTRUMENTATION_PID" 2>/dev/null || true
  fi
  INSTRUMENTATION_PID=""
  if [[ -n "$RUNNER_PID" ]] && kill -0 "$RUNNER_PID" 2>/dev/null; then
    kill -TERM "$RUNNER_PID" 2>/dev/null || true
    for _ in {1..50}; do
      kill -0 "$RUNNER_PID" 2>/dev/null || break
      sleep 0.1
    done
    if kill -0 "$RUNNER_PID" 2>/dev/null; then
      kill -KILL "$RUNNER_PID" 2>/dev/null || true
    fi
    wait "$RUNNER_PID" 2>/dev/null || true
  fi
  if [[ -n "$HEADLESS_MASTER_KEY_FILE" && -f "$HEADLESS_MASTER_KEY_FILE" ]]; then
    unlink "$HEADLESS_MASTER_KEY_FILE" 2>/dev/null || true
  fi
  HEADLESS_MASTER_KEY_FILE=""
  cleanup_device_artifacts
}
trap cleanup_all EXIT INT TERM

HEADLESS_MASTER_KEY_FILE="$(mktemp /private/tmp/gmofg-headless-master-key.XXXXXX)"
chmod 600 "$HEADLESS_MASTER_KEY_FILE"
# 未签名的 runner 直接访问 Keychain 可能弹授权框，因此由系统 security 命令导出当前用户主密钥。
# 文件权限强制为 0600，并由 EXIT trap 删除；任何日志和 instrumentation 参数都不得包含该值。
if ! security find-generic-password \
  -s com.gmofg.payment-proxy \
  -a secret-protection-master-key-v1 \
  -w >"$HEADLESS_MASTER_KEY_FILE" 2>/dev/null
then
  echo "unable to export the current-user Keychain master key for headless validation" >&2
  exit 1
fi
if ! LC_ALL=C tr -d '\n' <"$HEADLESS_MASTER_KEY_FILE" \
  | rg -q '^[0-9a-fA-F]{64}$'
then
  echo "headless Keychain master key has an unexpected format" >&2
  exit 1
fi

DEVICE_CREDIT_TID="$(read_device_setting CREDIT_TID)"
DEVICE_CONFIRM_CODE="$(read_device_setting CONFIRM_CODE)"
DEVICE_PAYMENT_PASSWORD="$(read_device_setting PASSWORD)"
if [[ -z "$DEVICE_CREDIT_TID" || -z "$DEVICE_CONFIRM_CODE" || -z "$DEVICE_PAYMENT_PASSWORD" ]]; then
  echo "required Payment settings are unavailable on the Android device" >&2
  exit 1
fi
DEVICE_SETTINGS_JSON="$(
  printf '{"creditTid":%s,"confirmCode":%s,"password":%s}' \
    "$(printf '%s' "$DEVICE_CREDIT_TID" | jq -Rs .)" \
    "$(printf '%s' "$DEVICE_CONFIRM_CODE" | jq -Rs .)" \
    "$(printf '%s' "$DEVICE_PAYMENT_PASSWORD" | jq -Rs .)"
)"
# 敏感材料通过 adb 标准输入写进测试包私有目录，而不是命令行参数。
# 命令行可能出现在进程列表和测试报告中；私有文件则使用 umask 077 并随后检查权限为 600。
adb -s "$GMOFG_DEVICE_SERIAL" shell \
  "run-as jp.gmofg.payment.test sh -c \
  'mkdir -p files && umask 077 && cat > files/gmofg-proxy-ca.der'" \
  <"$GMOFG_PROXY_CA_DER"
adb -s "$GMOFG_DEVICE_SERIAL" shell \
  "run-as jp.gmofg.payment.test sh -c \
  'mkdir -p files && umask 077 && cat > files/gmofg-client.p12'" \
  <"$GMOFG_CLIENT_P12"
printf '%s' "$DEVICE_SETTINGS_JSON" \
  | adb -s "$GMOFG_DEVICE_SERIAL" shell \
    "run-as jp.gmofg.payment.test sh -c \
    'mkdir -p files && umask 077 && cat > files/gmofg-device-settings.json'"
if adb -s "$GMOFG_DEVICE_SERIAL" shell \
  "run-as jp.gmofg.payment.test sh -c \
  '[ -s files/gmofg-proxy-ca.der ] &&
   [ -s files/gmofg-client.p12 ] &&
   [ -s files/gmofg-device-settings.json ] &&
   [ \"\$(stat -c %a files/gmofg-proxy-ca.der)\" = 600 ] &&
   [ \"\$(stat -c %a files/gmofg-client.p12)\" = 600 ] &&
   [ \"\$(stat -c %a files/gmofg-device-settings.json)\" = 600 ]'" \
  </dev/null >/dev/null 2>&1
then
  echo "private_material_files_valid=true" \
    >"$EVIDENCE_ROOT/security-checks.txt"
else
  echo "private_material_files_valid=false" \
    >"$EVIDENCE_ROOT/security-checks.txt"
  exit 1
fi
adb -s "$GMOFG_DEVICE_SERIAL" logcat -c >/dev/null 2>&1 || true

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

append_failure() {
  local scenario="$1"
  local template_id="$2"
  local reason="$3"
  jq -cn \
    --arg scenario "$scenario" \
    --arg template_id "$template_id" \
    --arg reason "$reason" \
    '{
      scenario: $scenario,
      template_id: (if $template_id == "none" then null else $template_id end),
      status: "FAIL",
      reason: $reason
    }' >>"$RESULTS_FILE"
}

run_probe() {
  # 一个场景对应一个独立目录、一个 Rust runner 进程和一次 Android instrumentation。
  # 这种隔离让失败证据不会被前后场景的日志、规则计数或运行时 epoch 污染。
  local scenario_json="$1"
  local scenario
  local template_id
  local expected_kind
  local scenario_dir
  local runner_log
  local instrumentation_log
  local device_log
  local adb_status
  local runner_status
  local result_json
  local rule_id
  local rule_hit_count
  local android_transport_outcome
  local android_elapsed_ms
  local completion_signal
  local ready_file
  local phase_file
  local runner_phase
  local adb_pid
  local request_count
  local instrumentation_deadline_seconds
  local expected_semantic
  local final_action
  local -a instrumentation_args

  scenario="$(jq -r '.id' <<<"$scenario_json")"
  template_id="$(jq -r '.template_id // "none"' <<<"$scenario_json")"
  expected_kind="$(jq -r '.android_expected_kind' <<<"$scenario_json")"
  scenario_dir="$EVIDENCE_ROOT/$scenario"
  runner_log="$scenario_dir/runner.txt"
  instrumentation_log="$scenario_dir/instrumentation.txt"
  device_log="$scenario_dir/device-logcat.txt"
  completion_signal="$scenario_dir/android-complete.signal"
  ready_file="$scenario_dir/runner-ready.signal"
  phase_file="$scenario_dir/runner-phase.txt"
  mkdir -p "$scenario_dir"
  rm -f "$completion_signal" "$ready_file" "$phase_file"

  GMOFG_APP_DATA_DIR="$GMOFG_APP_DATA_DIR" \
    GMOFG_ANDROID_COMPLETION_SIGNAL="$completion_signal" \
    GMOFG_HEADLESS_MASTER_KEY_FILE="$HEADLESS_MASTER_KEY_FILE" \
    GMOFG_HEADLESS_READY_FILE="$ready_file" \
    GMOFG_HEADLESS_PHASE_FILE="$phase_file" \
    "$RUNNER_BIN" "$scenario" >"$runner_log" 2>&1 &
  RUNNER_PID="$!"
  for ((index = 0; index < 1800; index += 1)); do
    if [[ -f "$ready_file" ]] \
      && rg -qx "scenario=$scenario" "$ready_file" 2>/dev/null
    then
      break
    fi
    if ! kill -0 "$RUNNER_PID" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  if [[ ! -f "$ready_file" ]] \
    || ! rg -qx "scenario=$scenario" "$ready_file" 2>/dev/null
  then
    runner_phase="$(tr -d '\n' <"$phase_file" 2>/dev/null || echo unknown)"
    append_failure "$scenario" "$template_id" \
      "runner did not become ready (phase=$runner_phase)"
    return 1
  fi

  instrumentation_args=(
    -e class jp.gmofg.payment.proxy.DllProxyRealDeviceTest#ruleScenarioThroughProxy
    -e scenario "$scenario"
    -e expectedKind "$expected_kind"
    -e proxyUrl "$GMOFG_PROXY_URL"
  )
  # instrumentation 参数只传“如何断言”的非敏感信息。证书和终端配置已经通过私有文件下发。
  if jq -e 'has("expected_status")' <<<"$scenario_json" >/dev/null; then
    instrumentation_args+=(
      -e expectedStatus "$(jq -r '.expected_status' <<<"$scenario_json")"
    )
  fi
  if jq -e 'has("expected_text")' <<<"$scenario_json" >/dev/null; then
    instrumentation_args+=(
      -e expectedText "$(jq -r '.expected_text' <<<"$scenario_json")"
    )
  fi
  if jq -e 'has("expected_header_name")' <<<"$scenario_json" >/dev/null; then
    instrumentation_args+=(
      -e expectedHeaderName "$(jq -r '.expected_header_name' <<<"$scenario_json")"
      -e expectedHeaderValue "$(jq -r '.expected_header_value' <<<"$scenario_json")"
    )
  fi
  if jq -e 'has("minimum_elapsed_ms")' <<<"$scenario_json" >/dev/null; then
    instrumentation_args+=(
      -e minimumElapsedMs "$(jq -r '.minimum_elapsed_ms' <<<"$scenario_json")"
    )
  fi
  if jq -e 'has("expected_sequence")' <<<"$scenario_json" >/dev/null; then
    instrumentation_args+=(
      -e expectedSequence "$(jq -r '.expected_sequence' <<<"$scenario_json")"
    )
  fi
  if jq -e 'has("expected_exception")' <<<"$scenario_json" >/dev/null; then
    instrumentation_args+=(
      -e expectedException "$(jq -r '.expected_exception' <<<"$scenario_json")"
    )
  fi
  if printf '%s\n' "${instrumentation_args[@]}" \
    | rg -qi 'clientP12|proxyCa|creditTid|confirmCode|password'
  then
    echo "instrumentation_sensitive_args_present=true" \
      >>"$EVIDENCE_ROOT/security-checks.txt"
    append_failure "$scenario" "$template_id" \
      "instrumentation argument list contains sensitive fields"
    return 1
  fi
  echo "instrumentation_sensitive_args_present=false" \
    >>"$EVIDENCE_ROOT/security-checks.txt"

  adb -s "$GMOFG_DEVICE_SERIAL" logcat -c >/dev/null 2>&1 || true
  adb -s "$GMOFG_DEVICE_SERIAL" shell am instrument -w -r \
    "${instrumentation_args[@]}" \
    jp.gmofg.payment.test/androidx.test.runner.AndroidJUnitRunner \
    >"$instrumentation_log" 2>&1 &
  adb_pid="$!"
  INSTRUMENTATION_PID="$adb_pid"
  request_count="$(jq -r '.request_count // 1' <<<"$scenario_json")"
  instrumentation_deadline_seconds="$((request_count * 100 + 60))"
  for ((index = 0; index < instrumentation_deadline_seconds * 10; index += 1)); do
    if ! kill -0 "$adb_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  set -e
  if kill -0 "$adb_pid" 2>/dev/null; then
    adb -s "$GMOFG_DEVICE_SERIAL" shell am force-stop jp.gmofg.payment.test \
      >/dev/null 2>&1 || true
    kill -TERM "$adb_pid" 2>/dev/null || true
    wait "$adb_pid" 2>/dev/null || true
    adb_status=124
  else
    set +e
    wait "$adb_pid"
    adb_status="$?"
    set -e
  fi
  INSTRUMENTATION_PID=""
  touch "$completion_signal"

  # Android 完成后仍要等待 Rust 输出 HEADLESS_CLEAN；只看到 Android OK 并不代表规则已经删除，
  # 若直接进入下一场景，命中次数和动作可能串场。
  if ! wait_for_log "$RUNNER_PID" "$runner_log" \
    "HEADLESS_CLEAN scenario=$scenario remaining_test_rules=0 created_rule_remaining=0" \
    1500; then
    cleanup_all
    RUNNER_PID=""
    append_failure "$scenario" "$template_id" "runner did not clean the scenario"
    return 1
  fi
  set +e
  wait "$RUNNER_PID"
  runner_status="$?"
  set -e
  RUNNER_PID=""

  adb -s "$GMOFG_DEVICE_SERIAL" logcat -d -v threadtime \
    | rg 'DllProxyRealDeviceTest|DLL_PROXY_SCENARIO_CONFIRMED' \
    >"$device_log" || true

  # 三层证据分别回答不同问题：instrumentation 是否通过、Rust 是否观察到预期内部语义、
  # 真机 logcat 是否留下对应场景确认标记。任何一层缺失都记为 FAIL。
  if [[ "$adb_status" != "0" ]] || ! rg -q 'OK \(1 test\)' "$instrumentation_log"; then
    append_failure "$scenario" "$template_id" "Android instrumentation failed"
    return 1
  fi
  if [[ "$runner_status" != "0" ]] || ! rg -q '^HEADLESS_RESULT ' "$runner_log"; then
    append_failure "$scenario" "$template_id" "Rust observation failed"
    return 1
  fi
  if ! rg -q "DLL_PROXY_SCENARIO_CONFIRMED scenario=$scenario" "$device_log"; then
    append_failure "$scenario" "$template_id" "Android outcome marker is missing"
    return 1
  fi

  result_json="$(sed -n 's/^HEADLESS_RESULT //p' "$runner_log" | tail -n 1)"
  jq -e \
    --argjson expected_hits "$(jq -r '.expected_rule_hits' <<<"$scenario_json")" \
    --argjson require_mtls "$(
      jq -r \
        'if has("require_mtls_summary") then .require_mtls_summary else true end' \
        <<<"$scenario_json"
    )" \
    --argjson expect_invalid "$(
      jq -r \
        'if has("expected_invalid_error") then .expected_invalid_error else false end' \
        <<<"$scenario_json"
    )" \
    --arg expected_invalid_field "$(
      jq -r '.expected_invalid_field // ""' <<<"$scenario_json"
    )" \
    --argjson expect_failed_trace "$(
      jq -r \
        'if has("expected_failed_trace") then .expected_failed_trace else false end' \
        <<<"$scenario_json"
    )" \
    --argjson expect_weak_network "$(
      jq -r 'if .batch == "G" then true else false end' <<<"$scenario_json"
    )" \
    '.action_effect_confirmed == true
      and (
        if $expected_hits == null
        then .rule_hit_count == null
        else .rule_hit_count == $expected_hits
        end
      )
      and (
        if $expected_hits == null
        then true
        else (.rule_trace | length > 0)
        end
      )
      and (
        if $require_mtls
        then (.tls_summary | startswith("TLS 1.2 mTLS"))
        else true
        end
      )
      and (.template_inventory.closed == true)
      and (
        if $expect_invalid
        then (
          .invalid_rejection.code == "RULE_INVALID"
          and .invalid_rejection.expected_fields == [$expected_invalid_field]
          and .invalid_rejection.actual_fields == [$expected_invalid_field]
          and .invalid_rejection.field_signature_exact == true
          and (.invalid_rejection.field_errors | keys) == [$expected_invalid_field]
        )
        else true
        end
      )
      and (
        if $expect_failed_trace
        then any(.rule_trace[]; contains("[未命中]"))
        else true
        end
      )
      and (
        if $expect_weak_network
        then (
          .action_semantic_confirmed == true
          and .reasonable_duration_confirmed == true
        )
        else true
        end
      )' \
    <<<"$result_json" >/dev/null
  rule_id="$(jq -r '.rule_id' <<<"$result_json")"
  rule_hit_count="$(jq -r '.rule_hit_count' <<<"$result_json")"
  expected_semantic="$(jq -r '.expected_semantic // "scenario-specific rule action"' <<<"$result_json")"
  final_action="$(jq -r '.final_action // empty' <<<"$result_json")"
  android_transport_outcome="$(
    rg "DLL_PROXY_SCENARIO_CONFIRMED scenario=$scenario" "$device_log" \
      | tail -n 1 \
      | sed -E 's/^.*outcome=([^ ]+).*$/\1/'
  )"
  android_elapsed_ms="$(
    rg "DLL_PROXY_SCENARIO_CONFIRMED scenario=$scenario" "$device_log" \
      | tail -n 1 \
      | sed -E 's/^.*elapsedMs=([0-9]+).*$/\1/'
  )"
  if [[ ! "$android_elapsed_ms" =~ ^[0-9]+$ ]]; then
    append_failure "$scenario" "$template_id" "Android elapsedMs marker is invalid"
    return 1
  fi
  LAST_ANDROID_ELAPSED_MS="$android_elapsed_ms"

  jq -cn \
    --arg scenario "$scenario" \
    --arg template_id "$template_id" \
    --arg rule_id "$rule_id" \
    --arg android_outcome "$expected_kind" \
    --arg android_transport_outcome "$android_transport_outcome" \
    --arg expected_semantic "$expected_semantic" \
    --arg final_action "$final_action" \
    --argjson android_elapsed_ms "$android_elapsed_ms" \
    --argjson rule_hit_count "$rule_hit_count" \
    '{
      scenario: $scenario,
      template_id: (if $template_id == "none" then null else $template_id end),
      status: "PASS",
      rule_id: (if $rule_id == "null" then null else $rule_id end),
      android_outcome: $android_outcome,
      android_transport_outcome: $android_transport_outcome,
      android_elapsed_ms: $android_elapsed_ms,
      expected_semantic: $expected_semantic,
      final_action: (if $final_action == "" then null else $final_action end),
      rule_hit_count: $rule_hit_count,
      trace_confirmed: true,
      cleanup_confirmed: true
    }' | tee -a "$RESULTS_FILE"
}

run_baseline() {
  local name="$1"
  local scenario_json
  scenario_json="$(
    jq -cn \
      --arg id "baseline" \
      --arg expected_kind "$(jq -r '.baseline.android_expected_kind' "$MATRIX_FILE")" \
      --argjson expected_hits "$(jq -r '.baseline.expected_rule_hits' "$MATRIX_FILE")" \
      '{
        id: $id,
        template_id: null,
        android_expected_kind: $expected_kind,
        expected_rule_hits: $expected_hits
      }'
  )"
  run_probe "$scenario_json"
  mv "$EVIDENCE_ROOT/baseline" "$EVIDENCE_ROOT/$name"
}

record_batch_recovery() {
  local batch="$1"
  run_baseline "baseline-after-batch-$batch"
  jq -cn \
    --arg batch "$batch" \
    --argjson android_elapsed_ms "$LAST_ANDROID_ELAPSED_MS" \
    '{
      batch: $batch,
      status: "PASS",
      android_outcome: "d48",
      android_elapsed_ms: $android_elapsed_ms
    }' >>"$BATCH_RECOVERY_ASSERTIONS_FILE"
}

run_baseline baseline-initial
current_batch=""
while IFS= read -r scenario_json <&3; do
  scenario_id="$(jq -r '.id' <<<"$scenario_json")"
  scenario_batch="$(jq -r '.batch' <<<"$scenario_json")"
  if [[ -n "$current_batch" && "$scenario_batch" != "$current_batch" ]]; then
    record_batch_recovery "$current_batch"
  fi
  current_batch="$scenario_batch"
  if [[ "$scenario_id" == "request-delay" || "$scenario_id" == "delay" ]]; then
    run_baseline "baseline-before-$scenario_id"
    baseline_before_elapsed_ms="$LAST_ANDROID_ELAPSED_MS"
    run_probe "$scenario_json"
    delayed_elapsed_ms="$LAST_ANDROID_ELAPSED_MS"
    run_baseline "baseline-after-$scenario_id"
    baseline_after_elapsed_ms="$LAST_ANDROID_ELAPSED_MS"
    if ((baseline_before_elapsed_ms > baseline_after_elapsed_ms)); then
      baseline_reference_elapsed_ms="$baseline_before_elapsed_ms"
    else
      baseline_reference_elapsed_ms="$baseline_after_elapsed_ms"
    fi
    delay_delta_ms="$((delayed_elapsed_ms - baseline_reference_elapsed_ms))"
    minimum_delta_ms=8500
    jq -cn \
      --arg scenario "$scenario_id" \
      --argjson baseline_before_elapsed_ms "$baseline_before_elapsed_ms" \
      --argjson baseline_after_elapsed_ms "$baseline_after_elapsed_ms" \
      --argjson baseline_reference_elapsed_ms "$baseline_reference_elapsed_ms" \
      --argjson delayed_elapsed_ms "$delayed_elapsed_ms" \
      --argjson delta_ms "$delay_delta_ms" \
      --argjson minimum_delta_ms "$minimum_delta_ms" \
      '{
        scenario: $scenario,
        baseline_before_elapsed_ms: $baseline_before_elapsed_ms,
        baseline_after_elapsed_ms: $baseline_after_elapsed_ms,
        baseline_reference_elapsed_ms: $baseline_reference_elapsed_ms,
        delayed_elapsed_ms: $delayed_elapsed_ms,
        delta_ms: $delta_ms,
        minimum_delta_ms: $minimum_delta_ms,
        status: (if $delta_ms >= $minimum_delta_ms then "PASS" else "FAIL" end)
      }' >>"$DELAY_ASSERTIONS_FILE"
    if ((delay_delta_ms < minimum_delta_ms)); then
      echo "delay delta for $scenario_id was ${delay_delta_ms}ms, expected at least ${minimum_delta_ms}ms" >&2
      exit 1
    fi
  else
    run_probe "$scenario_json"
  fi
done 3< <(
  jq -c \
    --arg batch "$GMOFG_BATCH" \
    --arg scenario "$GMOFG_SCENARIO" \
    '.scenarios[]
      | select(
          (.batch == $batch or $batch == "ALL")
          and .implementation == "ready"
          and ($scenario == "" or .id == $scenario)
        )' \
    "$MATRIX_FILE"
)
if [[ -n "$current_batch" ]]; then
  record_batch_recovery "$current_batch"
fi
cleanup_device_artifacts

TEST_PACKAGE_REMOVED=true
if adb -s "$GMOFG_DEVICE_SERIAL" shell pm path jp.gmofg.payment.test \
  2>/dev/null | rg -q '^package:'
then
  TEST_PACKAGE_REMOVED=false
fi
LISTENERS_CLEARED=true
if lsof -nP -iTCP:16127 -sTCP:LISTEN >/dev/null 2>&1 \
  || lsof -nP -iTCP:16627 -sTCP:LISTEN >/dev/null 2>&1
then
  LISTENERS_CLEARED=false
fi
SQLITE_HEADLESS_RULES="$(
  sqlite3 "$GMOFG_SQLITE_PATH" \
    "SELECT COUNT(*) FROM rules WHERE json LIKE '%headless-device-%';"
)"
SQLITE_FAULT_RULES="$(
  sqlite3 "$GMOFG_SQLITE_PATH" \
    "SELECT COUNT(*) FROM rules WHERE json LIKE '%\"description\":\"fault:%';"
)"

jq -s \
  --arg batch "$GMOFG_BATCH" \
  --arg selected_scenario "$GMOFG_SCENARIO" \
  --arg device_serial "$GMOFG_DEVICE_SERIAL" \
  --arg proxy_url_host_port "$GMOFG_PROXY_URL_HOST_PORT" \
  --arg source_digest "$SOURCE_DIGEST" \
  --arg runner_sha256 "$RUNNER_SHA256" \
  --arg android_apk_sha256 "$ANDROID_APK_SHA256" \
  --argjson private_material_files_valid "$(
    rg -q '^private_material_files_valid=true$' "$EVIDENCE_ROOT/security-checks.txt" \
      && echo true || echo false
  )" \
  --argjson sensitive_args_absent "$(
    ! rg -q '^instrumentation_sensitive_args_present=true$' \
      "$EVIDENCE_ROOT/security-checks.txt" && echo true || echo false
  )" \
  --argjson test_package_removed "$TEST_PACKAGE_REMOVED" \
  --argjson listeners_cleared "$LISTENERS_CLEARED" \
  --argjson expected_scenarios "$SELECTED_SCENARIO_COUNT" \
  --argjson sqlite_headless_rules "$SQLITE_HEADLESS_RULES" \
  --argjson sqlite_fault_rules "$SQLITE_FAULT_RULES" \
  --slurpfile delay_assertions "$DELAY_ASSERTIONS_FILE" \
  --slurpfile batch_recovery_assertions "$BATCH_RECOVERY_ASSERTIONS_FILE" \
  --slurpfile matrix "$MATRIX_FILE" \
  '. as $results
  | ([
      $matrix[0].scenarios[]
      | select(
          (.batch == $batch or $batch == "ALL")
          and .implementation == "ready"
          and ($selected_scenario == "" or .id == $selected_scenario)
        )
      | .id
    ] | sort) as $expected_ids
  | ([$results[] | select(.scenario != "baseline") | .scenario] | sort) as $executed_ids
  | ([
      $matrix[0].scenarios[]
      | select(
          (.batch == $batch or $batch == "ALL")
          and .implementation == "ready"
          and ($selected_scenario == "" or .id == $selected_scenario)
        )
      | .batch
    ] | unique | sort) as $expected_recovery_batches
  | ([$batch_recovery_assertions[] | .batch] | unique | sort) as $recovered_batches
  | {
    schema_version: 1,
    batch: $batch,
    device_serial: $device_serial,
    proxy_url_host_port: $proxy_url_host_port,
    status: (if all(.[]; .status == "PASS") then "PASS" else "FAIL" end),
    baseline_recovery: (
      ([.[] | select(.scenario == "baseline" and .status == "PASS")] | length) >= 2
    ),
    expected_scenarios: $expected_scenarios,
    executed_scenarios: ([.[] | select(.scenario != "baseline")] | length),
    expected_scenario_ids: $expected_ids,
    executed_scenario_ids: $executed_ids,
    scenario_id_set_match: ($expected_ids == $executed_ids),
    batch_recovery_complete: (
      $expected_recovery_batches == $recovered_batches
      and all($batch_recovery_assertions[]; .status == "PASS" and .android_outcome == "d48")
    ),
    source_digest: $source_digest,
    runner_sha256: $runner_sha256,
    android_test_apk_sha256: $android_apk_sha256,
    security_checks: {
      private_material_files_valid: $private_material_files_valid,
      sensitive_instrumentation_args_absent: $sensitive_args_absent,
      test_package_removed: $test_package_removed,
      listeners_cleared: $listeners_cleared
    },
    cleanup_checks: {
      sqlite_headless_rules: $sqlite_headless_rules,
      sqlite_fault_rules: $sqlite_fault_rules
    },
    batch_recovery_assertions: $batch_recovery_assertions,
    batch_summaries: [
      $matrix[0].scenarios
      | map(select(
          (.batch == $batch or $batch == "ALL")
          and .implementation == "ready"
          and ($selected_scenario == "" or .id == $selected_scenario)
        ))
      | group_by(.batch)[]
      | . as $definitions
      | {
          batch: $definitions[0].batch,
          expected: ($definitions | length),
          executed: ([
            $results[]
            | select(.scenario != "baseline")
            | .scenario as $id
            | select(any($definitions[]; .id == $id))
          ] | length)
        }
    ],
    delay_assertions: $delay_assertions,
    results: $results
  }' "$RESULTS_FILE" >"$EVIDENCE_ROOT/report.json"

jq -e \
  '.status == "PASS"
    and .baseline_recovery == true
    and .batch_recovery_complete == true
    and .executed_scenarios == .expected_scenarios
    and .scenario_id_set_match == true
    and (.android_test_apk_sha256 | test("^[0-9a-f]{64}$"))
    and all(.batch_summaries[]; .executed == .expected)
    and all(.delay_assertions[]; .status == "PASS")
    and .cleanup_checks.sqlite_headless_rules == 0
    and .cleanup_checks.sqlite_fault_rules == 0
    and all(.security_checks[]; . == true)' \
  "$EVIDENCE_ROOT/report.json" >/dev/null
echo "HEADLESS_DEVICE_MATRIX_OK batch=$GMOFG_BATCH evidence_root=$EVIDENCE_ROOT"
