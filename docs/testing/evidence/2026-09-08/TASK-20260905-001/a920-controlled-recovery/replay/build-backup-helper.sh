#!/bin/sh
set -eu
repo_dir=$(git rev-parse --show-toplevel)
replay_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
support_dir=$(mktemp -d "${TMPDIR:-/tmp}/a920-backup.XXXXXX")
mkdir -p "$support_dir/src"
cp "$replay_dir/RuntimePreferencesBackupInstrumentedTest-v2.kt" "$support_dir/src/RuntimePreferencesBackupInstrumentedTest.kt"
cat > "$support_dir/support.init.gradle" <<EOF
gradle.beforeProject { project ->
    if (project.path == ":app") {
        project.pluginManager.withPlugin("com.android.application") {
            project.extensions.getByName("android").sourceSets.getByName("androidTest").kotlin.srcDir("$support_dir/src")
            project.extensions.getByName("androidComponents").finalizeDsl { androidDsl ->
                androidDsl.buildTypes.getByName("debug").signingConfig = androidDsl.signingConfigs.getByName("stableCompanion")
            }
        }
    }
}
EOF
gradle -p "$repo_dir/android-companion" --no-daemon -I "$support_dir/support.init.gradle" :app:assembleDebugAndroidTest -x :app:buildRustAndroid
cp "$repo_dir/android-companion/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk" "$support_dir/stable-androidTest.apk"
printf '%s\n' "Prepared test APK: $support_dir/stable-androidTest.apk"
