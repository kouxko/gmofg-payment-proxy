#!/usr/bin/env bash
set -euo pipefail

test_root="$(mktemp -d)"
lock_dir="$test_root/task-manager.lock"
task_file="$test_root/task-state.txt"
index_file="$test_root/index-state.txt"

cleanup() {
  test -n "$test_root"
  test "$test_root" != "/"
  rm -rf -- "$test_root"
}
trap cleanup EXIT

write_owner() {
  local session_id="$1"
  local process_id="$2"
  local operation="$3"
  local task_id="$4"
  local recovered_from="$5"

  jq -n \
    --arg session_id "$session_id" \
    --argjson process_id "$process_id" \
    --arg operation "$operation" \
    --argjson task_id "$task_id" \
    --argjson recovered_from "$recovered_from" \
    '{
      schema_version: 1,
      session_id: $session_id,
      process_id: $process_id,
      acquired_at: "2026-08-25 22:26:57 +08:00",
      operation: $operation,
      task_id: $task_id,
      state: "active",
      recovered_from: $recovered_from
    }' >"$lock_dir/owner.json.pending"
  mv -- "$lock_dir/owner.json.pending" "$lock_dir/owner.json"
}

acquire_lock() {
  local session_id="$1"
  local process_id="$2"
  local operation="$3"
  local task_id="$4"
  local recovered_from="$5"

  if compgen -G "$test_root/task-manager.abandoned.*" >/dev/null; then
    return 4
  fi
  mkdir "$lock_dir" 2>/dev/null || return 1
  if compgen -G "$test_root/task-manager.abandoned.*" >/dev/null; then
    rmdir "$lock_dir"
    return 4
  fi
  write_owner "$session_id" "$process_id" "$operation" "$task_id" "$recovered_from"
}

acquire_recovery_lock() {
  local session_id="$1"
  local process_id="$2"
  local task_id="$3"
  local recovered_from="$4"

  mkdir "$lock_dir" 2>/dev/null || return 1
  write_owner "$session_id" "$process_id" 'recover' "$task_id" "$recovered_from"
}

write_recovery_owner() {
  local abandoned_dir="$1"
  local session_id="$2"
  local process_id="$3"
  local original_owner_state="$4"
  local original_owner_path="$5"
  local original_owner_raw_path="$6"
  local interrupted_recovery_locks="$7"
  local original_owner='null'

  if test "$original_owner_state" = 'valid'; then
    original_owner="$(jq -c . "$original_owner_path")"
  fi

  jq -n \
    --arg session_id "$session_id" \
    --argjson process_id "$process_id" \
    --arg original_owner_state "$original_owner_state" \
    --argjson original_owner "$original_owner" \
    --argjson original_owner_raw_path "$original_owner_raw_path" \
    --argjson interrupted_recovery_locks "$interrupted_recovery_locks" \
    '{
      schema_version: 1,
      session_id: $session_id,
      process_id: $process_id,
      acquired_at: "2026-08-25 22:54:57 +08:00",
      state: "active",
      original_owner_state: $original_owner_state,
      original_owner: $original_owner,
      original_owner_raw_path: $original_owner_raw_path,
      interrupted_recovery_locks: $interrupted_recovery_locks
    }' >"$abandoned_dir/recovery-owner.json.pending"
  mv -- \
    "$abandoned_dir/recovery-owner.json.pending" \
    "$abandoned_dir/recovery-owner.json"
}

interrupted_lock_record() {
  local interrupted_dir="$1"
  local relative_path="$2"
  local owner_state
  local owner='null'
  local owner_raw_path='null'

  if test ! -e "$interrupted_dir/owner.json"; then
    owner_state='missing'
  elif jq -e . "$interrupted_dir/owner.json" >/dev/null 2>&1; then
    owner_state='valid'
    owner="$(jq -c . "$interrupted_dir/owner.json")"
  else
    owner_state='damaged'
    mv -- "$interrupted_dir/owner.json" "$interrupted_dir/owner.damaged.raw"
    owner_raw_path="$(jq -Rn --arg path "$relative_path/owner.damaged.raw" '$path')"
  fi

  jq -cn \
    --arg path "$relative_path" \
    --arg owner_state "$owner_state" \
    --argjson owner "$owner" \
    --argjson owner_raw_path "$owner_raw_path" \
    '{
      path: $path,
      owner_state: $owner_state,
      owner: $owner,
      owner_raw_path: $owner_raw_path
    }'
}

recovery_lock_matches() {
  local abandoned_basename="$1"

  jq -e \
    --arg recovered_from "$abandoned_basename" \
    '.operation == "recover" and .recovered_from == $recovered_from' \
    "$lock_dir/owner.json" >/dev/null 2>&1
}

isolate_interrupted_recovery_lock() {
  local abandoned_dir="$1"
  local interrupted_name="$2"
  local interrupted_dir="$abandoned_dir/$interrupted_name"
  local abandoned_dirs=()
  local recovery_process_id

  shopt -s nullglob
  abandoned_dirs=("$test_root"/task-manager.abandoned.*)
  shopt -u nullglob
  test "${#abandoned_dirs[@]}" -eq 1 || return 10
  test "${abandoned_dirs[0]}" = "$abandoned_dir" || return 11
  test -d "$lock_dir" || return 12
  test -f "$abandoned_dir/recovery-owner.json" || return 13
  recovery_process_id="$(jq -r '.process_id // empty' "$abandoned_dir/recovery-owner.json")"
  if test -n "$recovery_process_id" && kill -0 "$recovery_process_id" 2>/dev/null; then
    return 14
  fi

  if test -e "$lock_dir/owner.json" && jq -e . "$lock_dir/owner.json" >/dev/null 2>&1; then
    recovery_lock_matches "$(basename "$abandoned_dir")" || return 15
  fi
  test ! -e "$interrupted_dir" || return 16

  mv -- "$lock_dir" "$interrupted_dir"
  interrupted_lock_record "$interrupted_dir" "$interrupted_name"
}

owner_matches() {
  local session_id="$1"
  local process_id="$2"

  jq -e \
    --arg session_id "$session_id" \
    --argjson process_id "$process_id" \
    '.session_id == $session_id and .process_id == $process_id and .acquired_at == "2026-08-25 22:26:57 +08:00"' \
    "$lock_dir/owner.json" >/dev/null
}

state_is_consistent() {
  cmp -s "$task_file" "$index_file"
}

release_lock() {
  local session_id="$1"
  local process_id="$2"

  owner_matches "$session_id" "$process_id" || return 2
  state_is_consistent || return 3
  rm -- "$lock_dir/owner.json"
  rmdir "$lock_dir"
}

printf 'before\n' >"$task_file"
printf 'before\n' >"$index_file"

acquire_lock 'owner-a' '41001' 'update' '"TASK-TEST-001"' 'null'
printf 'initial_acquire=PASS\n'

for attempt in 1 2 3; do
  if acquire_lock 'owner-b' '41002' 'create' 'null' 'null'; then
    printf 'unexpected_second_acquire=FAIL\n'
    exit 1
  fi
  printf 'wait_retry_%s=LOCKED\n' "$attempt"
done

if release_lock 'owner-b' '41002'; then
  printf 'wrong_owner_release=FAIL\n'
  exit 1
fi
printf 'wrong_owner_release=REFUSED\n'

printf 'partial\n' >"$task_file"
if release_lock 'owner-a' '41001'; then
  printf 'partial_state_release=FAIL\n'
  exit 1
fi
test -d "$lock_dir"
printf 'partial_state_release=REFUSED_LOCK_RETAINED\n'

printf 'before\n' >"$task_file"
release_lock 'owner-a' '41001'
printf 'owner_repair_and_release=PASS\n'

acquire_lock 'inactive-session' '99999999' 'close' '"TASK-TEST-002"' 'null'
if kill -0 99999999 2>/dev/null; then
  printf 'dead_process_check=FAIL\n'
  exit 1
fi
abandoned_dir="$test_root/task-manager.abandoned.valid"
mv -- "$lock_dir" "$abandoned_dir"
write_recovery_owner \
  "$abandoned_dir" 'recovery-session' "$$" 'valid' \
  "$abandoned_dir/owner.json" 'null' '[]'
jq -e \
  '.original_owner_state == "valid"
    and .original_owner.session_id == "inactive-session"
    and .original_owner_raw_path == null
    and .interrupted_recovery_locks == []' \
  "$abandoned_dir/recovery-owner.json" >/dev/null
if acquire_lock 'inserting-session' '41003' 'create' 'null' 'null'; then
  printf 'recovery_window_insertion=FAIL\n'
  exit 1
fi
test ! -e "$lock_dir"
printf 'recovery_window_insertion=REFUSED\n'
acquire_recovery_lock 'recovery-session' "$$" '"TASK-TEST-002"' '"task-manager.abandoned.valid"'
release_lock 'recovery-session' "$$"
rm -- "$abandoned_dir/owner.json" "$abandoned_dir/recovery-owner.json"
rmdir "$abandoned_dir"
printf 'inactive_owner_recovery=PASS\n'

mkdir "$lock_dir"
test ! -e "$lock_dir/owner.json"
test ! -e "$lock_dir/owner.json"
missing_owner_dir="$test_root/task-manager.abandoned.missing-owner"
mv -- "$lock_dir" "$missing_owner_dir"
write_recovery_owner \
  "$missing_owner_dir" 'recovery-session' "$$" 'missing' '' 'null' '[]'
jq -e \
  '.original_owner_state == "missing"
    and .original_owner == null
    and .original_owner_raw_path == null' \
  "$missing_owner_dir/recovery-owner.json" >/dev/null
if acquire_lock 'inserting-session' '41004' 'update' 'null' 'null'; then
  printf 'missing_owner_recovery_window=FAIL\n'
  exit 1
fi
test ! -e "$lock_dir"
printf 'missing_owner_recovery_window=REFUSED\n'
acquire_recovery_lock 'recovery-session' "$$" 'null' '"task-manager.abandoned.missing-owner"'
release_lock 'recovery-session' "$$"
rm -- "$missing_owner_dir/recovery-owner.json"
rmdir "$missing_owner_dir"
printf 'missing_owner_recovery=PASS\n'

mkdir "$lock_dir"
printf '{damaged owner bytes\n' >"$lock_dir/owner.json"
cp -- "$lock_dir/owner.json" "$test_root/damaged-owner.expected"
damaged_owner_dir="$test_root/task-manager.abandoned.damaged-owner"
mv -- "$lock_dir" "$damaged_owner_dir"
mv -- \
  "$damaged_owner_dir/owner.json" \
  "$damaged_owner_dir/original-owner.damaged.raw"
write_recovery_owner \
  "$damaged_owner_dir" 'recovery-session' "$$" 'damaged' '' \
  '"original-owner.damaged.raw"' '[]'
jq -e \
  '.original_owner_state == "damaged"
    and .original_owner == null
    and .original_owner_raw_path == "original-owner.damaged.raw"' \
  "$damaged_owner_dir/recovery-owner.json" >/dev/null
cmp -s \
  "$test_root/damaged-owner.expected" \
  "$damaged_owner_dir/original-owner.damaged.raw"
acquire_recovery_lock \
  'recovery-session' "$$" 'null' '"task-manager.abandoned.damaged-owner"'
release_lock 'recovery-session' "$$"
rm -- \
  "$damaged_owner_dir/original-owner.damaged.raw" \
  "$damaged_owner_dir/recovery-owner.json" \
  "$test_root/damaged-owner.expected"
rmdir "$damaged_owner_dir"
printf 'damaged_owner_raw_preserved=PASS\n'

mkdir "$lock_dir"
write_owner 'inactive-original' '99999999' 'close' '"TASK-TEST-003"' 'null'
recovery_died_before_lock="$test_root/task-manager.abandoned.recovery-died-before-lock"
mv -- "$lock_dir" "$recovery_died_before_lock"
write_recovery_owner \
  "$recovery_died_before_lock" 'inactive-recovery-a' '99999998' 'valid' \
  "$recovery_died_before_lock/owner.json" 'null' '[]'
recovery_takeover="$test_root/task-manager.abandoned.recovery-takeover"
mv -- "$recovery_died_before_lock" "$recovery_takeover"
write_recovery_owner \
  "$recovery_takeover" 'recovery-session-b' "$$" 'valid' \
  "$recovery_takeover/owner.json" 'null' '[]'
if acquire_lock 'inserting-session' '41006' 'create' 'null' 'null'; then
  printf 'recovery_owner_takeover_window=FAIL\n'
  exit 1
fi
test ! -e "$lock_dir"
acquire_recovery_lock 'recovery-session-b' "$$" '"TASK-TEST-003"' '"task-manager.abandoned.recovery-takeover"'
release_lock 'recovery-session-b' "$$"
rm -- "$recovery_takeover/owner.json" "$recovery_takeover/recovery-owner.json"
rmdir "$recovery_takeover"
printf 'recovery_died_before_main_lock_takeover=PASS\n'

mkdir "$lock_dir"
write_owner 'inactive-original' '99999999' 'close' '"TASK-TEST-004"' 'null'
recovery_died_after_lock="$test_root/task-manager.abandoned.recovery-died-after-lock"
mv -- "$lock_dir" "$recovery_died_after_lock"
write_recovery_owner \
  "$recovery_died_after_lock" 'inactive-recovery-c' '99999997' 'valid' \
  "$recovery_died_after_lock/owner.json" 'null' '[]'
acquire_recovery_lock 'inactive-recovery-c' '99999997' '"TASK-TEST-004"' '"task-manager.abandoned.recovery-died-after-lock"'
recovery_lock_matches 'task-manager.abandoned.recovery-died-after-lock'
first_interrupted_name='interrupted-recovery-lock.20260825T230000.recovery-session-d'
first_interrupted_dir="$recovery_died_after_lock/$first_interrupted_name"
first_interrupted_record="$(
  isolate_interrupted_recovery_lock \
    "$recovery_died_after_lock" "$first_interrupted_name"
)"
first_interrupted_records="$(
  jq -cn --argjson record "$first_interrupted_record" '[$record]'
)"
recovery_after_lock_takeover="$test_root/task-manager.abandoned.recovery-after-lock-takeover"
mv -- "$recovery_died_after_lock" "$recovery_after_lock_takeover"
write_recovery_owner \
  "$recovery_after_lock_takeover" 'recovery-session-d' "$$" 'valid' \
  "$recovery_after_lock_takeover/owner.json" 'null' \
  "$first_interrupted_records"
if acquire_lock 'inserting-session' '41007' 'update' 'null' 'null'; then
  printf 'interrupted_recovery_lock_window=FAIL\n'
  exit 1
fi
test ! -e "$lock_dir"
acquire_recovery_lock 'recovery-session-d' "$$" '"TASK-TEST-004"' '"task-manager.abandoned.recovery-after-lock-takeover"'
release_lock 'recovery-session-d' "$$"
rm -- \
  "$recovery_after_lock_takeover/$first_interrupted_name/owner.json" \
  "$recovery_after_lock_takeover/owner.json" \
  "$recovery_after_lock_takeover/recovery-owner.json"
rmdir "$recovery_after_lock_takeover/$first_interrupted_name"
rmdir "$recovery_after_lock_takeover"
printf 'recovery_died_after_main_lock_takeover=PASS\n'

mkdir "$lock_dir"
write_owner 'inactive-original' '99999999' 'close' '"TASK-TEST-005"' 'null'
multi_gen_one="$test_root/task-manager.abandoned.multi-gen-one"
mv -- "$lock_dir" "$multi_gen_one"
write_recovery_owner \
  "$multi_gen_one" 'inactive-recovery-one' '99999996' 'valid' \
  "$multi_gen_one/owner.json" 'null' '[]'

# First recovery process exits after mkdir and before owner.json exists.
mkdir "$lock_dir"
first_missing_name='interrupted-recovery-lock.20260825T231000.recovery-two'
first_missing_dir="$multi_gen_one/$first_missing_name"
first_missing_record="$(
  isolate_interrupted_recovery_lock "$multi_gen_one" "$first_missing_name"
)"
test "$(jq -r '.owner_state' <<<"$first_missing_record")" = 'missing'
multi_gen_two="$test_root/task-manager.abandoned.multi-gen-two"
mv -- "$multi_gen_one" "$multi_gen_two"
first_generation_records="$(
  jq -cn --argjson record "$first_missing_record" '[$record]'
)"
write_recovery_owner \
  "$multi_gen_two" 'inactive-recovery-two' '99999995' 'valid' \
  "$multi_gen_two/owner.json" 'null' "$first_generation_records"

# Second recovery process exits after writing a damaged owner.json.
mkdir "$lock_dir"
printf '{damaged recovery owner bytes\n' >"$lock_dir/owner.json"
cp -- "$lock_dir/owner.json" "$test_root/damaged-recovery-owner.expected"
second_damaged_name='interrupted-recovery-lock.20260825T232000.recovery-three'
second_damaged_dir="$multi_gen_two/$second_damaged_name"
second_damaged_record="$(
  isolate_interrupted_recovery_lock "$multi_gen_two" "$second_damaged_name"
)"
test "$(jq -r '.owner_state' <<<"$second_damaged_record")" = 'damaged'
cmp -s \
  "$test_root/damaged-recovery-owner.expected" \
  "$second_damaged_dir/owner.damaged.raw"
second_generation_records="$(
  jq -cn \
    --argjson first "$first_missing_record" \
    --argjson second "$second_damaged_record" \
    '[$first, $second]'
)"
multi_gen_three="$test_root/task-manager.abandoned.multi-gen-three"
mv -- "$multi_gen_two" "$multi_gen_three"
write_recovery_owner \
  "$multi_gen_three" 'recovery-session-three' "$$" 'valid' \
  "$multi_gen_three/owner.json" 'null' "$second_generation_records"
jq -e \
  '.interrupted_recovery_locks | length == 2
    and .[0].owner_state == "missing"
    and .[1].owner_state == "damaged"
    and .[0].path != .[1].path' \
  "$multi_gen_three/recovery-owner.json" >/dev/null
test -d "$multi_gen_three/$first_missing_name"
test -d "$multi_gen_three/$second_damaged_name"
cmp -s \
  "$test_root/damaged-recovery-owner.expected" \
  "$multi_gen_three/$second_damaged_name/owner.damaged.raw"
acquire_recovery_lock \
  'recovery-session-three' "$$" '"TASK-TEST-005"' \
  '"task-manager.abandoned.multi-gen-three"'
release_lock 'recovery-session-three' "$$"
rm -rf -- "$multi_gen_three"
rm -- "$test_root/damaged-recovery-owner.expected"
printf 'multi_generation_missing_and_damaged_recovery=PASS\n'

mkdir "$lock_dir"
write_owner 'inactive-recovery-mismatch' '99999994' 'recover' 'null' '"wrong-abandoned-dir"'
mismatch_outer="$test_root/task-manager.abandoned.expected"
mkdir "$mismatch_outer"
write_recovery_owner \
  "$mismatch_outer" 'inactive-recovery-mismatch' '99999994' \
  'missing' '' 'null' '[]'
mismatch_lock_before="$(shasum -a 256 "$lock_dir/owner.json")"
mismatch_recovery_before="$(shasum -a 256 "$mismatch_outer/recovery-owner.json")"
if isolate_interrupted_recovery_lock \
  "$mismatch_outer" 'interrupted-recovery-lock.20260825T232500.mismatch' >/dev/null; then
  printf 'mismatched_recovered_from=FAIL\n'
  exit 1
fi
test "$mismatch_lock_before" = "$(shasum -a 256 "$lock_dir/owner.json")"
test "$mismatch_recovery_before" = "$(shasum -a 256 "$mismatch_outer/recovery-owner.json")"
test ! -e "$mismatch_outer/interrupted-recovery-lock.20260825T232500.mismatch"
rm -- "$lock_dir/owner.json"
rmdir "$lock_dir"
rm -- "$mismatch_outer/recovery-owner.json"
rmdir "$mismatch_outer"
printf 'mismatched_recovered_from=REFUSED\n'

mkdir "$lock_dir"
mkdir "$test_root/task-manager.abandoned.one" "$test_root/task-manager.abandoned.two"
write_recovery_owner \
  "$test_root/task-manager.abandoned.one" 'inactive-recovery-one' '99999993' \
  'missing' '' 'null' '[]'
write_recovery_owner \
  "$test_root/task-manager.abandoned.two" 'inactive-recovery-two' '99999992' \
  'missing' '' 'null' '[]'
multiple_one_before="$(shasum -a 256 "$test_root/task-manager.abandoned.one/recovery-owner.json")"
multiple_two_before="$(shasum -a 256 "$test_root/task-manager.abandoned.two/recovery-owner.json")"
if isolate_interrupted_recovery_lock \
  "$test_root/task-manager.abandoned.one" \
  'interrupted-recovery-lock.20260825T232600.multiple' >/dev/null; then
  printf 'multiple_abandoned_dirs=FAIL\n'
  exit 1
fi
test -d "$lock_dir"
test "$multiple_one_before" = "$(shasum -a 256 "$test_root/task-manager.abandoned.one/recovery-owner.json")"
test "$multiple_two_before" = "$(shasum -a 256 "$test_root/task-manager.abandoned.two/recovery-owner.json")"
rm -- \
  "$test_root/task-manager.abandoned.one/recovery-owner.json" \
  "$test_root/task-manager.abandoned.two/recovery-owner.json"
rmdir \
  "$lock_dir" \
  "$test_root/task-manager.abandoned.one" \
  "$test_root/task-manager.abandoned.two"
printf 'multiple_abandoned_dirs=REFUSED\n'

mkdir "$lock_dir"
collision_outer="$test_root/task-manager.abandoned.collision"
mkdir "$collision_outer"
write_recovery_owner \
  "$collision_outer" 'inactive-recovery-collision' '99999991' \
  'missing' '' 'null' '[]'
write_owner \
  'inactive-recovery-collision' '99999991' 'recover' 'null' \
  '"task-manager.abandoned.collision"'
collision_target="$collision_outer/interrupted-recovery-lock.20260825T233000.collision"
mkdir "$collision_target"
collision_lock_before="$(shasum -a 256 "$lock_dir/owner.json")"
collision_recovery_before="$(shasum -a 256 "$collision_outer/recovery-owner.json")"
if isolate_interrupted_recovery_lock \
  "$collision_outer" "$(basename "$collision_target")" >/dev/null; then
  printf 'interrupted_target_collision=FAIL\n'
  exit 1
fi
test -d "$lock_dir"
test -d "$collision_target"
test "$collision_lock_before" = "$(shasum -a 256 "$lock_dir/owner.json")"
test "$collision_recovery_before" = "$(shasum -a 256 "$collision_outer/recovery-owner.json")"
rm -- "$lock_dir/owner.json" "$collision_outer/recovery-owner.json"
rmdir "$lock_dir" "$collision_target" "$collision_outer"
printf 'interrupted_target_collision=REFUSED\n'

acquire_lock 'post-recovery-session' '41005' 'create' 'null' 'null'
release_lock 'post-recovery-session' '41005'
printf 'normal_acquire_after_recovery=PASS\n'

printf 'result=PASS\n'
