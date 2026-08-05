package com.interceptproxy.vpn

import java.security.MessageDigest
import org.json.JSONArray
import org.json.JSONObject

/** Android 当前安装包快照。目标应用只按包名与 UID 识别，不校验 APK 签名。 */
data class PackageSnapshot(
    val packageName: String,
    val uid: Int,
)

/** Profile 保存的目标应用。 */
data class TargetPackage(
    val packageName: String,
    val uid: Int,
)

/** Kotlin 生命周期层真正需要读取的 Profile 字段。其余弱网字段保持在 [rawJson] 中交给 Rust。 */
data class CompanionProfile(
    val rawJson: String,
    val targetPackages: List<TargetPackage>,
    val expectedProxyRouteCount: Int,
    val autoResumeAfterReboot: Boolean,
    val mtu: Int,
)

/** 桌面端生成并由设备实际装载的代理路由运行事实。 */
data class ProxyRuntimeFacts(
    val profileFingerprint: String,
    val routeFingerprint: String,
    val routeCount: Int,
)

object ProxyRuntimeParser {
    fun parse(profileJson: String, rawJson: String): ProxyRuntimeFacts {
        val profile = JSONObject(profileJson)
        val root = JSONObject(rawJson)
        val routes = root.getJSONArray("routes")
        val routeSource = root.getJSONArray("route_source")
        val declaredCount = root.getInt("route_count")
        require(declaredCount == routes.length()) { "代理路由数量与运行元数据不一致" }
        require(declaredCount == routeSource.length()) { "代理路由来源数量与运行元数据不一致" }
        val actualProfileFingerprint = sha256(canonicalJson(profile))
        // 运行指纹必须覆盖 Android 真正交给 Rust 数据面的规范化路由，而不只是
        // Workspace 中的可移植来源。否则 proxy_host/proxy_port 或 DNS 解析结果被
        // 错配时，来源仍然正确却会被误判为同一次激活。
        val actualRouteFingerprint = sha256(canonicalJson(routes))
        return ProxyRuntimeFacts(
            profileFingerprint = root.getString("profile_fingerprint").also {
                require(it == actualProfileFingerprint) { "Profile 运行指纹与实际配置不一致" }
            },
            routeFingerprint = root.getString("route_fingerprint").also {
                require(it == actualRouteFingerprint) { "代理路由运行指纹与实际配置不一致" }
            },
            routeCount = declaredCount,
        )
    }

    internal fun sha256(value: String): String = MessageDigest.getInstance("SHA-256")
        .digest(value.toByteArray(Charsets.UTF_8))
        .joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }

    /**
     * 生成与 Rust 端完全一致的稳定 JSON 表示。
     *
     * `JSONObject.toString()` 会把 `/` 写成 `\/`，Rust 的 `serde_json` 则保留 `/`，
     * 因而 CIDR 和 URL 会在内容相同的情况下得到不同指纹。这里显式排序对象键，
     * 数组保持原顺序，并按 JSON 必需规则转义字符串，使两端能够可靠核对运行配置。
     */
    internal fun canonicalJson(value: Any?): String = when {
        value == null || value === JSONObject.NULL -> "null"
        value is JSONObject -> value.keys().asSequence().toList().sorted().joinToString(
            prefix = "{",
            postfix = "}",
            separator = ",",
        ) { key -> "${quoteJsonString(key)}:${canonicalJson(value.get(key))}" }
        value is JSONArray -> (0 until value.length()).joinToString(
            prefix = "[",
            postfix = "]",
            separator = ",",
        ) { index -> canonicalJson(value.get(index)) }
        value is String -> quoteJsonString(value)
        value is Boolean || value is Number -> value.toString()
        else -> error("不支持生成运行指纹的 JSON 类型：${value::class.java.name}")
    }

    private fun quoteJsonString(value: String): String = buildString(value.length + 2) {
        append('"')
        value.forEach { character ->
            when (character) {
                '"' -> append("\\\"")
                '\\' -> append("\\\\")
                '\b' -> append("\\b")
                '\u000C' -> append("\\f")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> {
                    if (character.code < 0x20) {
                        append("\\u")
                        append(character.code.toString(16).padStart(4, '0'))
                    } else {
                        append(character)
                    }
                }
            }
        }
        append('"')
    }
}

/**
 * 解析由 Rust DTO 生成的 JSON。
 *
 * Kotlin 不计算弱网语义，只读取建立 Android TUN 必需的应用列表与恢复开关。
 * `weak_network.path_mtu` 是 Rust 要模拟的网络路径 MTU，不能拿来设置 TUN 自身 MTU；
 * 否则 Android 内核会提前分片/缩小 TCP 段，Rust 就无法验证 PMTU 黑洞或 ICMP 信号。
 */
object CompanionProfileParser {
    fun parse(rawJson: String): CompanionProfile {
        val root = JSONObject(rawJson)
        val targets = root.getJSONArray("target_applications").mapObjects { target ->
            TargetPackage(
                packageName = target.getString("package_name"),
                uid = target.getInt("uid"),
            )
        }
        return CompanionProfile(
            rawJson = rawJson,
            targetPackages = targets,
            expectedProxyRouteCount = root.optJSONArray("proxy_routes")?.length() ?: 0,
            autoResumeAfterReboot = root.optBoolean("auto_resume_after_reboot", false),
            mtu = TUN_MTU,
        )
    }

    private const val TUN_MTU = 1_280
}

/** Rust JNI 使用的安装清单 JSON，字段名与 InstalledApplication 完全一致。 */
fun List<PackageSnapshot>.toInventoryJson(): String = JSONArray().also { array ->
    forEach { snapshot ->
        array.put(
            JSONObject()
                .put("package_name", snapshot.packageName)
                .put("uid", snapshot.uid),
        )
    }
}.toString()

private fun JSONArray.mapObjects(transform: (JSONObject) -> TargetPackage): List<TargetPackage> =
    (0 until length()).map { index -> transform(getJSONObject(index)) }
