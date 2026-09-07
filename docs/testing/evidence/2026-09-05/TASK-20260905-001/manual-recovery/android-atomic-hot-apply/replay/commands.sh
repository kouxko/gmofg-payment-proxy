#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../../../../../.." && pwd)"
gradle_bin="/Users/codin/.gradle/wrapper/dists/gradle-9.4.1-bin/arn2x92ynaizyzdaamcbpbhtj/gradle-9.4.1/bin/gradle"
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"

cd "$repository_root/android-companion"
"$gradle_bin" --no-daemon :app:testDebugUnitTest \
  --tests com.interceptproxy.vpn.ControlLeaseRecoveryTest \
  --tests com.interceptproxy.vpn.ControlProtocolTest
"$gradle_bin" --no-daemon :app:testDebugUnitTest
"$gradle_bin" --no-daemon :app:assembleDebugAndroidTest

cd "$repository_root"
PATH="$(dirname "$gradle_bin"):$PATH" bash scripts/build-android-companion.sh
