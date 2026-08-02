package com.interceptproxy.vpn

import android.content.pm.PackageManager
import android.os.Build
import java.security.MessageDigest

/** 每次启动前从 PackageManager 重建包名、签名和 UID 清单。 */
object PackageInventory {
    @Suppress("DEPRECATION")
    fun collect(packageManager: PackageManager): List<PackageSnapshot> =
        installedApplications(packageManager).map { app ->
            val signingCertificates = runCatching {
                val packageInfo = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    packageManager.getPackageInfo(
                        app.packageName,
                        PackageManager.PackageInfoFlags.of(PackageManager.GET_SIGNING_CERTIFICATES.toLong()),
                    )
                } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    packageManager.getPackageInfo(app.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
                } else {
                    // API 26/27 不理解 GET_SIGNING_CERTIFICATES，必须回退到旧签名标志。
                    packageManager.getPackageInfo(app.packageName, PackageManager.GET_SIGNATURES)
                }
                val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    val signingInfo = requireNotNull(packageInfo.signingInfo) {
                        "PackageManager 未返回 ${app.packageName} 的签名信息"
                    }
                    // 只比较当前 APK signer；证书轮换历史不应让同签名升级误判为替换。
                    signingInfo.apkContentsSigners
                } else {
                    @Suppress("DEPRECATION")
                    requireNotNull(packageInfo.signatures) {
                        "PackageManager 未返回 ${app.packageName} 的旧版签名信息"
                    }
                }
                signatures.map { signature -> signature.toByteArray() }
            }.getOrNull()
            snapshot(app.packageName, app.uid, signingCertificates)
        }

    /** 读取失败也必须保留包与 UID；空摘要会让 Rust 启动校验拒绝不可验证的应用。 */
    internal fun snapshot(
        packageName: String,
        uid: Int,
        signingCertificates: List<ByteArray>?,
    ): PackageSnapshot = PackageSnapshot(
        packageName = packageName,
        signingSha256 = signingCertificates
            ?.takeIf { it.isNotEmpty() }
            ?.map(::sha256)
            ?.sorted()
            ?.joinToString("+")
            .orEmpty(),
        uid = uid,
    )

    @Suppress("DEPRECATION")
    private fun installedApplications(packageManager: PackageManager) =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            packageManager.getInstalledApplications(PackageManager.ApplicationInfoFlags.of(0))
        } else {
            packageManager.getInstalledApplications(0)
        }

    private fun sha256(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256")
            .digest(bytes)
            .joinToString(":") { byte -> "%02X".format(byte) }
}
