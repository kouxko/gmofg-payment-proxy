#!/usr/bin/env bash
set -euo pipefail

# 把纯 Rust Android 数据面构建成 APK 可识别的四 ABI 动态库。
#
# 不依赖 cargo-ndk，避免桌面安装包额外携带构建工具。CI/开发机只需要已安装的 Rust
# target 与 Android NDK。可以通过 RUST_ANDROID_ABIS=arm64-v8a 只构建单一 ABI；默认
# 构建发布门禁要求的全部 ABI。

script_dir="$(cd "$(dirname "$0")" && pwd)"
companion_dir="$(cd "$script_dir/.." && pwd)"
repo_dir="$(cd "$companion_dir/.." && pwd)"
manifest="$repo_dir/src-tauri/Cargo.toml"
target_dir="$companion_dir/build/rust-target"
jni_dir="$companion_dir/app/src/main/jniLibs"
android_api="${ANDROID_MIN_API:-26}"

if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
  ndk_dir="$ANDROID_NDK_HOME"
elif [[ -n "${ANDROID_HOME:-}" && -d "$ANDROID_HOME/ndk" ]]; then
  ndk_dir="$(find "$ANDROID_HOME/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)"
elif [[ -d "$HOME/Library/Android/sdk/ndk" ]]; then
  ndk_dir="$(find "$HOME/Library/Android/sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)"
else
  echo "找不到 Android NDK；请设置 ANDROID_NDK_HOME 或 ANDROID_HOME。" >&2
  exit 1
fi

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64|Darwin-x86_64) host_tag="darwin-x86_64" ;;
  Linux-x86_64) host_tag="linux-x86_64" ;;
  *) echo "当前主机不受 Android NDK 脚本支持：$(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

toolchain="$ndk_dir/toolchains/llvm/prebuilt/$host_tag/bin"
abis="${RUST_ANDROID_ABIS:-arm64-v8a armeabi-v7a x86_64 x86}"

build_abi() {
  local abi="$1"
  local rust_target clang_prefix
  case "$abi" in
    arm64-v8a) rust_target="aarch64-linux-android"; clang_prefix="aarch64-linux-android" ;;
    armeabi-v7a) rust_target="armv7-linux-androideabi"; clang_prefix="armv7a-linux-androideabi" ;;
    x86_64) rust_target="x86_64-linux-android"; clang_prefix="x86_64-linux-android" ;;
    x86) rust_target="i686-linux-android"; clang_prefix="i686-linux-android" ;;
    *) echo "不支持的 Android ABI：$abi" >&2; exit 1 ;;
  esac

  rustup target add "$rust_target" >/dev/null
  local linker="$toolchain/${clang_prefix}${android_api}-clang"
  if [[ ! -x "$linker" ]]; then
    echo "找不到 NDK linker：$linker" >&2
    exit 1
  fi

  local linker_env="CARGO_TARGET_$(printf '%s' "$rust_target" | tr '[:lower:]-' '[:upper:]_')_LINKER"
  env \
    CARGO_TARGET_DIR="$target_dir" \
    "$linker_env=$linker" \
    RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384" \
    cargo build \
      --manifest-path "$manifest" \
      --package intercept-proxy-android-engine \
      --target "$rust_target" \
      --release

  mkdir -p "$jni_dir/$abi"
  cp \
    "$target_dir/$rust_target/release/libintercept_proxy_android_engine.so" \
    "$jni_dir/$abi/libintercept_proxy_android_engine.so"
}

for abi in $abis; do
  build_abi "$abi"
done
