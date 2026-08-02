#!/usr/bin/env bash
set -euo pipefail

# 验证 APK 不是“Gradle 任务成功但不可交付”的空壳：包名、签名、四 ABI、zipalign 和
# Rust ELF 的 16 KiB LOAD 对齐必须同时成立。

apk=""
expected_cert_sha256=""
release_mode=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --expected-cert-sha256)
      [[ $# -ge 2 ]] || { echo "--expected-cert-sha256 缺少参数" >&2; exit 2; }
      expected_cert_sha256="$2"
      shift 2
      ;;
    --release)
      release_mode=true
      shift
      ;;
    -*)
      echo "未知参数: $1" >&2
      exit 2
      ;;
    *)
      [[ -z "$apk" ]] || { echo "只能验证一个 APK" >&2; exit 2; }
      apk="$1"
      shift
      ;;
  esac
done

if [[ -z "$apk" || ! -f "$apk" ]]; then
  echo "用法: $0 <android-companion.apk> [--release --expected-cert-sha256 <SHA-256>]" >&2
  exit 2
fi
if $release_mode && [[ -z "$expected_cert_sha256" ]]; then
  echo "release 门禁必须提供预期签名证书 SHA-256。" >&2
  exit 2
fi

android_sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
build_tools_dir="$(find "$android_sdk/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)"
ndk_dir="${ANDROID_NDK_HOME:-$(find "$android_sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)}"

signing_report="$("$build_tools_dir/apksigner" verify --verbose --print-certs "$apk")"
printf '%s\n' "$signing_report"

# release 不只检查“存在某个签名”，还必须只有一个 signer，且证书指纹与 CI 配置的
# 生产证书一致。比较前移除冒号/空白并统一大小写，兼容 GitHub Secret 的常见写法。
if [[ -n "$expected_cert_sha256" ]]; then
  signer_count="$(printf '%s\n' "$signing_report" | sed -n 's/^Number of signers: //p')"
  actual_cert_sha256="$(printf '%s\n' "$signing_report" |
    sed -n 's/^Signer #1 certificate SHA-256 digest: //p')"
  normalize_digest() { printf '%s' "$1" | tr -d ':[:space:]' | tr '[:upper:]' '[:lower:]'; }
  if [[ "$signer_count" != "1" ||
        "$(normalize_digest "$actual_cert_sha256")" != "$(normalize_digest "$expected_cert_sha256")" ]]; then
    echo "APK signer 与预期 release 证书不一致。" >&2
    exit 1
  fi
fi

if $release_mode && printf '%s\n' "$signing_report" |
  grep -Fq "CN=Android Debug"; then
  echo "release APK 禁止使用 Android Debug 证书。" >&2
  exit 1
fi

"$build_tools_dir/aapt" dump badging "$apk" | grep -F "package: name='com.interceptproxy.vpn'"
"$build_tools_dir/zipalign" -c -P 16 -v 4 "$apk"

case "$(uname -s)-$(uname -m)" in
  Darwin-*) host_tag="darwin-x86_64" ;;
  Linux-x86_64) host_tag="linux-x86_64" ;;
  *) echo "不支持的验证主机: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

readelf="$ndk_dir/toolchains/llvm/prebuilt/$host_tag/bin/llvm-readelf"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT
unzip -q "$apk" 'lib/*/libintercept_proxy_android_engine.so' -d "$temp_dir"

for abi in arm64-v8a armeabi-v7a x86_64 x86; do
  library="$temp_dir/lib/$abi/libintercept_proxy_android_engine.so"
  if [[ ! -f "$library" ]]; then
    echo "APK 缺少 ABI $abi 的 Rust 数据面。" >&2
    exit 1
  fi
  if ! "$readelf" -lW "$library" | awk '
    $1 == "LOAD" {
      found = 1
      if ($NF != "0x4000" && $NF != "0x8000" && $NF != "0x10000" && $NF != "0x20000") bad = 1
    }
    END { exit !(found && !bad) }
  '; then
    echo "$abi 的 ELF LOAD 段未全部按至少 16 KiB 对齐。" >&2
    exit 1
  fi
done

echo "Android Companion APK 门禁通过: $apk"
