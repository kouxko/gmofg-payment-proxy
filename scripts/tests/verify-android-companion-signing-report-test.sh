#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_dir/../.." && pwd)"
fixtures="$script_dir/fixtures"

# shellcheck source=../verify-android-companion.sh
source "$repository_root/scripts/verify-android-companion.sh"

expected="c92ff3da8cb6520775e028abfc1d57d746949e7d83a038ec7626c99b67544db2"

assert_digest() {
  local fixture="$1"
  local actual
  actual="$(parse_signing_report "$(<"$fixture")")"
  [[ "$actual" == "$expected" ]] || {
    echo "Unexpected digest for $fixture: $actual" >&2
    exit 1
  }
}

assert_parse_failure() {
  local fixture="$1"
  local error_file
  error_file="$(mktemp)"
  if parse_signing_report "$(<"$fixture")" >/dev/null 2>"$error_file"; then
    echo "Expected signer report parsing to fail: $fixture" >&2
    rm -f "$error_file"
    exit 1
  fi
  grep -Fq "APK 签名报告解析失败" "$error_file" || {
    echo "Missing clear parse error for $fixture" >&2
    rm -f "$error_file"
    exit 1
  }
  rm -f "$error_file"
}

assert_digest "$fixtures/apksigner-signer-numbered.txt"
assert_digest "$fixtures/apksigner-v3-signer.txt"
assert_digest "$fixtures/apksigner-v3-signer-colon.txt"
assert_parse_failure "$fixtures/apksigner-multiple-signers.txt"
assert_parse_failure "$fixtures/apksigner-missing-digest.txt"

echo "Android Companion signer report parser regression tests passed."
