package com.interceptproxy.vpn

import android.content.pm.PackageManager
import android.os.Build

/** 每次启动前从 PackageManager 重建包名与 UID 清单。 */
object PackageInventory {
    fun collect(packageManager: PackageManager): List<PackageSnapshot> =
        installedApplications(packageManager).map { app ->
            snapshot(app.packageName, app.uid)
        }

    /** Profile 与设备清单均只使用包名和 UID，不读取或比较应用签名。 */
    internal fun snapshot(
        packageName: String,
        uid: Int,
    ): PackageSnapshot = PackageSnapshot(
        packageName = packageName,
        uid = uid,
    )

    @Suppress("DEPRECATION")
    private fun installedApplications(packageManager: PackageManager) =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            packageManager.getInstalledApplications(PackageManager.ApplicationInfoFlags.of(0))
        } else {
            packageManager.getInstalledApplications(0)
        }
}
