#!/usr/bin/env bash
set -euo pipefail

# 把已经通过 Android 构建/签名门禁的 Companion APK 放入 Tauri 固定资源位置。
# 参数必须是明确 APK 路径；脚本不会猜测或降级到另一个变体，避免发布包误带调试 APK。

if [[ $# -ne 1 ]]; then
  echo "用法: $0 <已验证的 companion.apk>" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_dir="$(cd "$script_dir/.." && pwd)"
source_apk="$1"
destination_dir="$repo_dir/src-tauri/resources"
destination_apk="$destination_dir/android-companion.apk"

if [[ ! -f "$source_apk" ]]; then
  echo "Companion APK 不存在: $source_apk" >&2
  exit 1
fi

mkdir -p "$destination_dir"
cp "$source_apk" "$destination_apk"
echo "$destination_apk"
