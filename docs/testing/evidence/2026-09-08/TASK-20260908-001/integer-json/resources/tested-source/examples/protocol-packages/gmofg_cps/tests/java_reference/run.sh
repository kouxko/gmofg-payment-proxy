#!/bin/sh
set -eu

JBR_HOME=${JBR_HOME:-/Applications/Android Studio.app/Contents/jbr/Contents/Home}
CPS_REPO=${CPS_REPO:-/Users/codin/Code/jp_gmofg_cps}
GSON_JAR=${GSON_JAR:-$HOME/.gradle/caches/modules-2/files-2.1/com.google.code.gson/gson/2.9.0/8a1167e089096758b49f9b34066ef98b2f4b37aa/gson-2.9.0.jar}
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE=${FIXTURE:-$SCRIPT_DIR/../fixtures/java-signatures.json}
MESSAGE_SOURCE=$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/Message.java
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/gmofg-cps-java-oracle.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT HUP INT TERM

verify_sha() {
    expected=$1
    path=$2
    actual=$(shasum -a 256 "$path" | awk '{print $1}')
    if [ "$actual" != "$expected" ]; then
        echo "SHA-256 mismatch: $path" >&2
        echo "expected $expected" >&2
        echo "actual   $actual" >&2
        exit 1
    fi
}

verify_sha a75ddc02f78b3853739a35b9950292fa4f8e748f1529e462f27377891d460c60 "$MESSAGE_SOURCE"
verify_sha 42a6a60a0fbfbd52851439e7050319dd72180682f7efbe7fa7dca5de101aea7e "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/Meta.java"
verify_sha a11b131af1604ab3dc0ff5062315a4aa1c15f349d40c1c0e4a711f2deb14e3d4 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/BaseData.java"
verify_sha d22074e2663267c157ba13440e8ca3d7849dec1d0c3ebf9ee7e11ada50195186 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/BaseResult.java"
verify_sha 116729740a790f131d5fbea45bb753ccb38be4030a762cd1ae94d4472d9b2444 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/Pay.java"
verify_sha 83b18200173e6e8bc2321e13a1b3cca3379bb841309197a6769ff9031ade26e0 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/PayData.java"
verify_sha 2e599bc41ea7f2b6d9e8b9145765059a2c7a50c7b2af6b0b7f600e5b903bcc45 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/Confirm.java"
verify_sha 94b92108c8c0d54a1aab2cef06213b21cf0f580eacca7828cf1a4583c6c3257f "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/ConfirmData.java"
verify_sha 4e2c91df4a756a577732bd4535107bef7f03d49764d91f30798b0eded131f7a7 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/Refund.java"
verify_sha 8a4cdbbd8f67e6b020b32f2aa3ed6313cc3bd2d3a6a953a542d0b53ae8b02977 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/RefundData.java"
verify_sha 9414869ee32d0361a182830e5862969fef4de99ffd7923cc5cd30047e5cae606 "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/Reverse.java"
verify_sha d254441c7e771900f643ac68f7a44b69d6f65662060730609847aae3ae4ec40d "$CPS_REPO/app/src/main/java/jp/gmofg/cps/https/msg/ReverseData.java"
verify_sha c96d60551331a196dac54b745aa642cd078ef89b6f267146b705f2c2cbef052d "$GSON_JAR"

# The signature implementation below is extracted directly from Message.java.
# The single substitution only captures the exact pre-MD5 string; it does not
# alter field selection, ordering, conversion, hashing, or casing.
sed -n '305,474p' "$MESSAGE_SOURCE" \
    | sed 's/return md5(stringBuilder.toString()).toUpperCase();/lastCanonical = stringBuilder.toString(); return md5(lastCanonical).toUpperCase();/' \
    > "$WORK_DIR/message-methods.java"

{
    sed -n '1,$p' "$SCRIPT_DIR/OracleMain.prefix"
    sed -n '1,$p' "$WORK_DIR/message-methods.java"
    sed -n '1,$p' "$SCRIPT_DIR/OracleMain.suffix"
} > "$WORK_DIR/OracleMain.java"

"$JBR_HOME/bin/javac" -encoding UTF-8 -cp "$GSON_JAR" -d "$WORK_DIR/classes" "$WORK_DIR/OracleMain.java"

if [ "${1:-}" = "--generate" ]; then
    "$JBR_HOME/bin/java" -Dfile.encoding=UTF-8 -cp "$WORK_DIR/classes:$GSON_JAR" OracleMain --generate
else
    "$JBR_HOME/bin/java" -Dfile.encoding=UTF-8 -cp "$WORK_DIR/classes:$GSON_JAR" OracleMain --verify "$FIXTURE"
fi
