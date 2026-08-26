#!/bin/zsh

set -eu

scenario_file=${1:-docs/testing/evidence/2026-08-26/TASK-20260826-002/DOC-GOV-006/inputs/state-scenarios.json}

jq -e . "$scenario_file" >/dev/null

aggregate_result() {
  local scenario=$1
  local lifecycle target cleanup any_action
  lifecycle=$(jq -r '.lifecycle' <<<"$scenario")
  target=$(jq -r '.target' <<<"$scenario")
  cleanup=$(jq -r '.cleanup' <<<"$scenario")
  any_action=$(jq -r '.any_action' <<<"$scenario")

  if [[ "$lifecycle" != "FINAL" ]]; then
    print -r -- null
    return
  fi

  if [[ "$target" == "FAILED" || "$cleanup" == "FAILED" ]] || \
    jq -e '.prerequisites | index("FAILED") != null' <<<"$scenario" >/dev/null; then
    print -r -- FAILED
    return
  fi

  if [[ "$any_action" == "false" ]]; then
    print -r -- NOT_RUN
    return
  fi

  if [[ "$target" == "INCONCLUSIVE" || "$cleanup" == "INCONCLUSIVE" ]] || \
    jq -e '.prerequisites | index("INCONCLUSIVE") != null' <<<"$scenario" >/dev/null; then
    print -r -- INCONCLUSIVE
    return
  fi

  if [[ "$target" == "VERIFIED" && ("$cleanup" == "VERIFIED" || "$cleanup" == "N/A") ]] && \
    jq -e 'all(.prerequisites[]; . == "VERIFIED" or . == "N/A")' <<<"$scenario" >/dev/null; then
    print -r -- VERIFIED
    return
  fi

  print -r -- INCONCLUSIVE
}

while IFS= read -r scenario; do
  actual=$(aggregate_result "$scenario")
  expected=$(jq -r 'if .expected == null then "null" else .expected end' <<<"$scenario")
  [[ "$actual" == "$expected" ]]
  print -r -- "PASS aggregate $(jq -r '.id' <<<"$scenario"): $actual"
done < <(jq -c '.aggregation[]' "$scenario_file")

transition_allowed() {
  case "$1:$2" in
    RESERVED:RUNNING|RESERVED:FINAL|RUNNING:FINAL) print -r -- true ;;
    *) print -r -- false ;;
  esac
}

while IFS= read -r transition; do
  from=$(jq -r '.from' <<<"$transition")
  to=$(jq -r '.to' <<<"$transition")
  actual=$(transition_allowed "$from" "$to")
  expected=$(jq -r '.allowed' <<<"$transition")
  [[ "$actual" == "$expected" ]]
  print -r -- "PASS transition $from->$to: $actual"
done < <(jq -c '.transitions[]' "$scenario_file")

route_action() {
  local scenario=$1
  local existing conditions active upgrade
  existing=$(jq -r '.existing_task' <<<"$scenario")
  conditions=$(jq -r '.entry_conditions_met' <<<"$scenario")
  active=$(jq -r '.qv_active' <<<"$scenario")
  upgrade=$(jq -r '.upgrade_triggered' <<<"$scenario")

  if [[ "$active" == "true" && "$upgrade" == "true" ]]; then
    print -r -- FINALIZE_QV_THEN_CREATE_OR_REUSE_TASK
  elif [[ "$existing" == "true" ]]; then
    print -r -- USE_EXISTING_TASK_NO_QV
  elif [[ "$conditions" != "true" ]]; then
    print -r -- CREATE_TASK_NO_QV
  else
    print -r -- CREATE_QV
  fi
}

while IFS= read -r scenario; do
  actual=$(route_action "$scenario")
  expected=$(jq -r '.expected' <<<"$scenario")
  [[ "$actual" == "$expected" ]]
  print -r -- "PASS routing $(jq -r '.id' <<<"$scenario"): $actual"
done < <(jq -c '.routing[]' "$scenario_file")

# These scenarios use real temporary directories, atomic mkdir locks, separate
# processes, and persisted JSON records. They verify file-state behavior rather
# than only enumerating expected transitions.
scenario_root=$(mktemp -d "${TMPDIR:-/tmp}/qv-state-scenarios.XXXXXX")
state_root="$scenario_root/state"
qv_root="$state_root/quick-validations/2026-08-26"
index_root="$state_root/index"
lock_dir="$state_root/task-manager.lock"
mkdir -p "$qv_root" "$index_root"

cleanup_scenario_root() {
  rm -rf -- "$scenario_root"
}
trap cleanup_scenario_root EXIT INT TERM

acquire_lock() {
  local owner=$1
  while ! mkdir "$lock_dir" 2>/dev/null; do
    sleep 0.01
  done
  jq -n --arg owner "$owner" '{owner: $owner}' >"$lock_dir/owner.json"
}

release_lock() {
  rm -f -- "$lock_dir/owner.json"
  rmdir "$lock_dir"
}

next_qv_id() {
  local max_id=0 candidate suffix
  setopt local_options null_glob
  for candidate in "$qv_root"/QV-20260826-*; do
    suffix=${candidate:t}
    suffix=${suffix##*-}
    if (( 10#$suffix > max_id )); then
      max_id=$((10#$suffix))
    fi
  done
  printf 'QV-20260826-%03d' $((max_id + 1))
}

write_initial_record() {
  local qv_id=$1 session_id=$2 process_id=$3 lifecycle=$4 derived_from=$5
  local qv_dir="$qv_root/$qv_id"
  mkdir "$qv_dir"
  jq -n \
    --arg qv_id "$qv_id" \
    --arg lifecycle "$lifecycle" \
    --arg session_id "$session_id" \
    --argjson process_id "$process_id" \
    --arg derived_from "$derived_from" \
    '{
      validation_id: $qv_id,
      lifecycle: $lifecycle,
      result: null,
      cleanup: null,
      runner: {
        session_id: $session_id,
        process_id: $process_id,
        heartbeat_at: "2026-08-26 12:30:00 +08:00"
      },
      derived_from: (if $derived_from == "" then null else $derived_from end)
    }' >"$qv_dir/metadata.json"
  jq '{validation_id, lifecycle, result, cleanup, runner, derived_from}' \
    "$qv_dir/metadata.json" >"$index_root/$qv_id.json"
}

allocate_qv() {
  local session_id=$1 lifecycle=$2 result_file=$3 derived_from=${4:-}
  local record_process=${5:-false}
  local process_id=null qv_id
  if [[ "$record_process" == "true" ]]; then
    process_id=$(sh -c 'printf "%s" "$PPID"')
  fi
  acquire_lock "$session_id"
  qv_id=$(next_qv_id)
  write_initial_record "$qv_id" "$session_id" "$process_id" "$lifecycle" "$derived_from"
  print -r -- "$qv_id" >"$result_file"
  release_lock
}

take_recovery_ownership() {
  local qv_id=$1 recovery_session=$2 reason=$3
  local metadata="$qv_root/$qv_id/metadata.json"
  local index="$index_root/$qv_id.json"
  local temp_metadata="$metadata.recovery"
  local temp_index="$index.recovery"
  acquire_lock "$recovery_session"
  [[ "$(jq -r '.lifecycle' "$metadata")" != "FINAL" ]]
  jq \
    --arg recovery_session "$recovery_session" \
    --arg reason "$reason" \
    '.recovery = {
       previous_runner: .runner,
       reason: $reason,
       taken_at: "2026-08-26 12:31:00 +08:00"
     }
     | .runner = {
         session_id: $recovery_session,
         process_id: null,
         heartbeat_at: "2026-08-26 12:31:00 +08:00"
       }' "$metadata" >"$temp_metadata"
  mv "$temp_metadata" "$metadata"
  jq '{validation_id, lifecycle, result, cleanup, runner, derived_from}' \
    "$metadata" >"$temp_index"
  mv "$temp_index" "$index"
  [[ "$(jq -r '.runner.session_id' "$metadata")" == "$recovery_session" ]]
  [[ "$(jq -r '.runner.session_id' "$index")" == "$recovery_session" ]]
  release_lock
}

runner_write_fact() {
  local qv_id=$1 session_id=$2 fact=$3
  local metadata="$qv_root/$qv_id/metadata.json"
  local temp_metadata="$metadata.fact"
  [[ "$(jq -r '.runner.session_id' "$metadata")" == "$session_id" ]] || return 42
  jq --arg fact "$fact" '.last_runtime_fact = $fact' "$metadata" >"$temp_metadata"
  mv "$temp_metadata" "$metadata"
}

save_cleanup_fact_without_lock() {
  local qv_id=$1 recovery_session=$2
  local metadata="$qv_root/$qv_id/metadata.json"
  local temp_metadata="$metadata.cleanup"
  [[ ! -d "$lock_dir" ]]
  [[ "$(jq -r '.runner.session_id' "$metadata")" == "$recovery_session" ]]
  jq '.cleanup_audit = {
        lock_held: false,
        command: "inspect declared temporary object",
        actual: "no retained temporary object",
        status: "VERIFIED"
      }' "$metadata" >"$temp_metadata"
  mv "$temp_metadata" "$metadata"
}

finalize_qv() {
  local qv_id=$1 session_id=$2
  local metadata="$qv_root/$qv_id/metadata.json"
  local index="$index_root/$qv_id.json"
  local temp_metadata="$metadata.final"
  local temp_index="$index.final"
  acquire_lock "$session_id"
  if [[ "$(jq -r '.runner.session_id' "$metadata")" != "$session_id" ]] || \
    [[ "$(jq -r '.runner.session_id' "$index")" != "$session_id" ]] || \
    [[ "$(jq -r '.lifecycle' "$metadata")" == "FINAL" ]]; then
    release_lock
    return 43
  fi
  jq '.lifecycle = "FINAL"
      | .result = "INCONCLUSIVE"
      | .cleanup = .cleanup_audit.status' "$metadata" >"$temp_metadata"
  mv "$temp_metadata" "$metadata"
  jq '{validation_id, lifecycle, result, cleanup, runner, derived_from}' \
    "$metadata" >"$temp_index"
  mv "$temp_index" "$index"
  release_lock
}

mkdir "$qv_root/QV-20260826-001"

allocation_a="$scenario_root/allocation-a.txt"
allocation_b="$scenario_root/allocation-b.txt"
allocate_qv allocator-a RUNNING "$allocation_a" &
allocator_a_pid=$!
allocate_qv allocator-b RUNNING "$allocation_b" &
allocator_b_pid=$!
wait "$allocator_a_pid"
wait "$allocator_b_pid"
actual_allocations=$(sort "$allocation_a" "$allocation_b" | jq -R . | jq -s -c .)
expected_allocations=$(jq -c '.file_state.expected.concurrent_allocations' "$scenario_file")
[[ "$actual_allocations" == "$expected_allocations" ]]
print -r -- "PASS real concurrent allocation: $actual_allocations"

reserved_id_file="$scenario_root/reserved-id.txt"
allocate_qv original-reserved RESERVED "$reserved_id_file" "" true &
reserved_creator_pid=$!
wait "$reserved_creator_pid"
reserved_id=$(<"$reserved_id_file")
reserved_runner_pid=$(jq -r '.runner.process_id' "$qv_root/$reserved_id/metadata.json")
! kill -0 "$reserved_runner_pid" 2>/dev/null
take_recovery_ownership "$reserved_id" recovery-reserved inactive_runner
[[ "$(jq -r '.recovery.previous_runner.session_id' "$qv_root/$reserved_id/metadata.json")" == \
  "original-reserved" ]]
if runner_write_fact "$reserved_id" original-reserved stale-write; then
  print -u2 -r -- "original RESERVED runner unexpectedly wrote after recovery"
  exit 1
fi
save_cleanup_fact_without_lock "$reserved_id" recovery-reserved
finalize_qv "$reserved_id" recovery-reserved
[[ "$(jq -r '.lifecycle' "$qv_root/$reserved_id/metadata.json")" == "FINAL" ]]
print -r -- "PASS RESERVED exit, recovery ownership persisted, stale runner rejected: $reserved_id"

running_id_file="$scenario_root/running-id.txt"
allocate_qv original-running RUNNING "$running_id_file" "" true &
running_creator_pid=$!
wait "$running_creator_pid"
running_id=$(<"$running_id_file")
running_runner_pid=$(jq -r '.runner.process_id' "$qv_root/$running_id/metadata.json")
! kill -0 "$running_runner_pid" 2>/dev/null
take_recovery_ownership "$running_id" recovery-running inactive_runner
save_cleanup_fact_without_lock "$running_id" recovery-running

finalizer_a="$scenario_root/finalizer-a.txt"
finalizer_b="$scenario_root/finalizer-b.txt"
(
  if finalize_qv "$running_id" recovery-running; then
    print -r -- SUCCESS >"$finalizer_a"
  else
    print -r -- REJECTED >"$finalizer_a"
  fi
) &
finalizer_a_pid=$!
(
  if finalize_qv "$running_id" recovery-running; then
    print -r -- SUCCESS >"$finalizer_b"
  else
    print -r -- REJECTED >"$finalizer_b"
  fi
) &
finalizer_b_pid=$!
wait "$finalizer_a_pid"
wait "$finalizer_b_pid"
finalizer_results=$(sort "$finalizer_a" "$finalizer_b" | tr '\n' ' ' | sed 's/ $//')
[[ "$finalizer_results" == "REJECTED SUCCESS" ]]
[[ "$(jq -r '.lifecycle' "$qv_root/$running_id/metadata.json")" == "FINAL" ]]
print -r -- "PASS RUNNING exit and two-finalizer race: $finalizer_results"

repeat_id_file="$scenario_root/repeat-id.txt"
allocate_qv repeat-runner RUNNING "$repeat_id_file" "$running_id"
repeat_id=$(<"$repeat_id_file")
[[ "$repeat_id" != "$running_id" ]]
[[ "$(jq -r '.derived_from' "$qv_root/$repeat_id/metadata.json")" == "$running_id" ]]
print -r -- "PASS repeat execution creates new QV with derived_from: $repeat_id <- $running_id"

expected_repeat=$(jq -r '.file_state.expected.repeat_execution' "$scenario_file")
[[ "$expected_repeat" == "NEW_QV_WITH_DERIVED_FROM" ]]
