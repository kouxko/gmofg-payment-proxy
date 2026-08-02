package com.interceptproxy.vpn

import org.json.JSONArray
import org.json.JSONObject

/** Android 当前安装包的可信快照。 */
data class PackageSnapshot(
    val packageName: String,
    val signingSha256: String,
    val uid: Int,
)

/** Profile 保存的目标应用身份。 */
data class TargetPackage(
    val packageName: String,
    val signingSha256: String,
    val uid: Int,
)

/** Kotlin 生命周期层真正需要读取的 Profile 字段。其余弱网字段保持在 [rawJson] 中交给 Rust。 */
data class CompanionProfile(
    val rawJson: String,
    val targetPackages: List<TargetPackage>,
    val autoResumeAfterReboot: Boolean,
    val mtu: Int,
)

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
                signingSha256 = target.getString("signing_sha256"),
                uid = target.getInt("uid"),
            )
        }
        return CompanionProfile(
            rawJson = rawJson,
            targetPackages = targets,
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
                .put("signing_sha256", snapshot.signingSha256)
                .put("uid", snapshot.uid),
        )
    }
}.toString()

private fun JSONArray.mapObjects(transform: (JSONObject) -> TargetPackage): List<TargetPackage> =
    (0 until length()).map { index -> transform(getJSONObject(index)) }
