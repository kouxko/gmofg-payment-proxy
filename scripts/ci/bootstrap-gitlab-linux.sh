#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
case "$mode" in
  android|quality) ;;
  *)
    echo "usage: source scripts/ci/bootstrap-gitlab-linux.sh <android|quality>" >&2
    return 2 2>/dev/null || exit 2
    ;;
esac

repository_root="${CI_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
tools_root="$repository_root/.ci-cache/tools"
export CARGO_HOME="${CARGO_HOME:-$repository_root/.ci-cache/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$repository_root/.ci-cache/rustup}"
export DENO_INSTALL="${DENO_INSTALL:-$tools_root/deno}"
export DENO_DIR="${DENO_DIR:-$repository_root/.ci-cache/deno-cache}"
export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$repository_root/.ci-cache/gradle-user-home}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$repository_root/.ci-cache/android-sdk}"
export ANDROID_HOME="$ANDROID_SDK_ROOT"

mkdir -p \
  "$tools_root" \
  "$CARGO_HOME" \
  "$RUSTUP_HOME" \
  "$DENO_INSTALL" \
  "$DENO_DIR" \
  "$GRADLE_USER_HOME" \
  "$ANDROID_SDK_ROOT"

export PATH="$DENO_INSTALL/bin:$CARGO_HOME/bin:$PATH"

install_deno() {
  local archive="$tools_root/deno-$DENO_VERSION-linux-x64.zip"
  if [[ ! -f "$archive" ]] || ! printf '%s  %s\n' "$DENO_LINUX_SHA256" "$archive" |
    sha256sum --check --status; then
    curl --fail --show-error --silent --location \
      "https://github.com/denoland/deno/releases/download/v$DENO_VERSION/deno-x86_64-unknown-linux-gnu.zip" \
      --output "$archive"
  fi
  printf '%s  %s\n' "$DENO_LINUX_SHA256" "$archive" |
    sha256sum --check --status
  mkdir -p "$DENO_INSTALL/bin"
  unzip -q -o "$archive" -d "$DENO_INSTALL/bin"
  test "$("$DENO_INSTALL/bin/deno" --version | sed -n '1s/^deno //p')" = "$DENO_VERSION"
  deno --version
}

install_rust() {
  local installer="$tools_root/rustup-init-$RUSTUP_VERSION-linux-x64"
  if [[ ! -f "$installer" ]] || ! printf '%s  %s\n' "$RUSTUP_LINUX_SHA256" "$installer" |
    sha256sum --check --status; then
    curl --proto '=https' --tlsv1.2 --fail --show-error --silent --location \
      "https://static.rust-lang.org/rustup/archive/$RUSTUP_VERSION/x86_64-unknown-linux-gnu/rustup-init" \
      --output "$installer"
  fi
  printf '%s  %s\n' "$RUSTUP_LINUX_SHA256" "$installer" |
    sha256sum --check --status
  chmod +x "$installer"
  "$installer" -y --no-modify-path --profile minimal --default-toolchain none
  rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal
  rustup default "$RUST_TOOLCHAIN"
  rustc --version
  cargo --version
}

install_gradle() {
  local gradle_home="$tools_root/gradle-$GRADLE_VERSION"
  local archive="$tools_root/gradle-$GRADLE_VERSION-bin.zip"
  if [[ ! -f "$archive" ]] || ! printf '%s  %s\n' "$GRADLE_SHA256" "$archive" |
    sha256sum --check --status; then
    curl --fail --show-error --silent --location \
      "https://services.gradle.org/distributions/gradle-$GRADLE_VERSION-bin.zip" \
      --output "$archive"
  fi
  printf '%s  %s\n' "$GRADLE_SHA256" "$archive" | sha256sum --check --status
  unzip -q -o "$archive" -d "$tools_root"

  export GRADLE_HOME="$gradle_home"
  export PATH="$GRADLE_HOME/bin:$PATH"
  gradle --version
}

install_android_sdk() {
  local command_line_tools_version="$ANDROID_COMMAND_LINE_TOOLS_VERSION"
  local tools_archive="$tools_root/commandlinetools-linux-$command_line_tools_version.zip"
  local tools_home="$tools_root/android-command-line-tools-$command_line_tools_version"
  local sdkmanager="$tools_home/cmdline-tools/bin/sdkmanager"

  if [[ ! -f "$tools_archive" ]] || ! printf '%s  %s\n' "$ANDROID_COMMAND_LINE_TOOLS_SHA256" "$tools_archive" |
    sha256sum --check --status; then
    curl --fail --show-error --silent --location \
      "https://dl.google.com/android/repository/commandlinetools-linux-${command_line_tools_version}_latest.zip" \
      --output "$tools_archive"
  fi
  printf '%s  %s\n' "$ANDROID_COMMAND_LINE_TOOLS_SHA256" "$tools_archive" |
    sha256sum --check --status
  mkdir -p "$tools_home"
  unzip -q -o "$tools_archive" -d "$tools_home"

  export PATH="$(dirname "$sdkmanager"):$ANDROID_SDK_ROOT/platform-tools:$PATH"
  set +o pipefail
  yes | "$sdkmanager" --sdk_root="$ANDROID_SDK_ROOT" --licenses >/dev/null
  local license_status=$?
  set -o pipefail
  if [[ $license_status -ne 0 ]]; then
    echo "Android SDK license acceptance failed." >&2
    return "$license_status"
  fi
  "$sdkmanager" --sdk_root="$ANDROID_SDK_ROOT" \
    "platform-tools" \
    "platforms;android-$ANDROID_PLATFORM" \
    "build-tools;$ANDROID_BUILD_TOOLS_VERSION" \
    "ndk;$ANDROID_NDK_VERSION"
  export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/$ANDROID_NDK_VERSION"
  test -d "$ANDROID_NDK_HOME"
}

install_deno
install_rust

if [[ "$mode" == "android" ]]; then
  install_gradle
  install_android_sdk
else
  rustup component add llvm-tools-preview --toolchain "$RUST_TOOLCHAIN"
  rustup target add wasm32-wasip2 --toolchain "$RUST_TOOLCHAIN"
  if ! cargo llvm-cov --version 2>/dev/null | grep -F "cargo-llvm-cov $CARGO_LLVM_COV_VERSION" >/dev/null; then
    cargo install cargo-llvm-cov --version "$CARGO_LLVM_COV_VERSION" --locked
  fi
  cargo llvm-cov --version
fi
