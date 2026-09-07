# GMOFG CPS Java signature oracle

This oracle compiles the CPS `Message.java` signing implementation directly from
the checked-out Android project and evaluates synthetic downstream response
models with Gson 2.9.0. It never loads application settings, merchant data,
terminal data, or credentials.

Run verification from the payment-proxy repository root:

```sh
sh examples/protocol-packages/gmofg_cps/tests/java_reference/run.sh
```

Regenerate the fixture for review:

```sh
sh examples/protocol-packages/gmofg_cps/tests/java_reference/run.sh --generate \
  > examples/protocol-packages/gmofg_cps/tests/fixtures/java-signatures.json
```

The script requires the Android Studio JBR and the already cached Gson 2.9.0
JAR. It forces `-Dfile.encoding=UTF-8` and the Java runner rejects any other
default charset.

## Source extraction

`run.sh` verifies every source before compilation. The signing methods are
extracted byte-for-byte from lines 305-474 of:

- `/Users/codin/Code/jp_gmofg_cps/app/src/main/java/jp/gmofg/cps/https/msg/Message.java`
  — SHA-256 `a75ddc02f78b3853739a35b9950292fa4f8e748f1529e462f27377891d460c60`

One observation-only substitution stores `stringBuilder.toString()` immediately
before the original MD5 call. The original field traversal, filtering, sorting,
Java conversion, MD5 calculation, and uppercase conversion remain unchanged.

The minimal response classes in `OracleMain.prefix` mirror the private fields and
inheritance from these verified sources:

- `Meta.java` — `42a6a60a0fbfbd52851439e7050319dd72180682f7efbe7fa7dca5de101aea7e`
- `BaseData.java` — `a11b131af1604ab3dc0ff5062315a4aa1c15f349d40c1c0e4a711f2deb14e3d4`
- `BaseResult.java` — `d22074e2663267c157ba13440e8ca3d7849dec1d0c3ebf9ee7e11ada50195186`
- `Pay.java` — `116729740a790f131d5fbea45bb753ccb38be4030a762cd1ae94d4472d9b2444`
- `PayData.java` — `83b18200173e6e8bc2321e13a1b3cca3379bb841309197a6769ff9031ade26e0`
- `Confirm.java` — `2e599bc41ea7f2b6d9e8b9145765059a2c7a50c7b2af6b0b7f600e5b903bcc45`
- `ConfirmData.java` — `94b92108c8c0d54a1aab2cef06213b21cf0f580eacca7828cf1a4583c6c3257f`
- `Refund.java` — `4e2c91df4a756a577732bd4535107bef7f03d49764d91f30798b0eded131f7a7`
- `RefundData.java` — `8a4cdbbd8f67e6b020b32f2aa3ed6313cc3bd2d3a6a953a542d0b53ae8b02977`
- `Reverse.java` — `9414869ee32d0361a182830e5862969fef4de99ffd7923cc5cd30047e5cae606`
- `ReverseData.java` — `d254441c7e771900f643ac68f7a44b69d6f65662060730609847aae3ae4ec40d`

Gson source:

- `$HOME/.gradle/caches/modules-2/files-2.1/com.google.code.gson/gson/2.9.0/8a1167e089096758b49f9b34066ef98b2f4b37aa/gson-2.9.0.jar`
  — SHA-256 `c96d60551331a196dac54b745aa642cd078ef89b6f267146b705f2c2cbef052d`

The fixture contains only synthetic values. Each successful row records the raw
JSON body, space-preserving token, exact canonical Java string, and uppercase
MD5. Rejected Gson conversions record `expect_error` and the root Java exception
type.
