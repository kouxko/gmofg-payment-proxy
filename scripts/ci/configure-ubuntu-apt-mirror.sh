#!/usr/bin/env bash
set -euo pipefail

mirror="${APT_MIRROR_URL:-http://mirrors.aliyun.com/ubuntu}"

if [[ -f /etc/apt/sources.list.d/ubuntu.sources ]]; then
  sed -i -E \
    "s#https?://(archive|security)\.ubuntu\.com/ubuntu/?#$mirror#g" \
    /etc/apt/sources.list.d/ubuntu.sources
elif [[ -f /etc/apt/sources.list ]]; then
  sed -i -E \
    "s#https?://(archive|security)\.ubuntu\.com/ubuntu/?#$mirror#g" \
    /etc/apt/sources.list
else
  echo "Ubuntu apt source configuration was not found." >&2
  exit 1
fi

echo "Ubuntu apt mirror: $mirror"
