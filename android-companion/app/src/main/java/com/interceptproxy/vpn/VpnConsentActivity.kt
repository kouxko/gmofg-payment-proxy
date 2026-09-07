package com.interceptproxy.vpn

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle

/** 只承接 Android 系统 VPN 授权，不创建自定义授权 UI。 */
class VpnConsentActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestVpnPermission()
    }

    private fun requestVpnPermission() {
        val permissionIntent = VpnService.prepare(this)
        when (VpnConsentPolicy.afterPrepare(permissionIntent != null)) {
            VpnConsentNextStep.Finish -> {
                // 授权页只负责授权。启动必须由桌面端随后发送的版本化 start/apply 命令
                // 携带当次规范化路由，避免复用旧的 ADB reverse 端点。
                finish()
            }
            VpnConsentNextStep.OpenSystemDialog -> {
                @Suppress("DEPRECATION")
                startActivityForResult(checkNotNull(permissionIntent), REQUEST_VPN_PERMISSION)
            }
        }
    }

    @Deprecated("Android 系统 VPN 授权仍通过 Activity result 返回")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        // 无论同意还是拒绝都只结束授权 Activity；不得从授权回调启动 Service。
        if (requestCode == REQUEST_VPN_PERMISSION) {
            check(
                VpnConsentPolicy.afterResult(resultCode == RESULT_OK) ==
                    VpnConsentNextStep.Finish,
            )
            finish()
        }
    }

    companion object {
        private const val REQUEST_VPN_PERMISSION = 100
    }
}

internal enum class VpnConsentNextStep {
    OpenSystemDialog,
    Finish,
}

/** 授权流程不包含“启动 Service”分支；运行态只能由桌面 start/apply 创建。 */
internal object VpnConsentPolicy {
    fun afterPrepare(permissionRequired: Boolean): VpnConsentNextStep =
        if (permissionRequired) VpnConsentNextStep.OpenSystemDialog else VpnConsentNextStep.Finish

    fun afterResult(@Suppress("UNUSED_PARAMETER") granted: Boolean): VpnConsentNextStep =
        VpnConsentNextStep.Finish
}
