package com.interceptproxy.vpn

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.widget.Toast

/** 只承接 Android 系统 VPN 授权，不创建自定义授权 UI。 */
class VpnConsentActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestVpnPermission()
    }

    private fun requestVpnPermission() {
        val permissionIntent = VpnService.prepare(this)
        if (permissionIntent == null) {
            startConfiguredProfileIfPresent()
            return
        }
        @Suppress("DEPRECATION")
        startActivityForResult(permissionIntent, REQUEST_VPN_PERMISSION)
    }

    @Deprecated("Android 系统 VPN 授权仍通过 Activity result 返回")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST_VPN_PERMISSION && resultCode == RESULT_OK) {
            startConfiguredProfileIfPresent()
        } else {
            Toast.makeText(this, R.string.vpn_permission_denied, Toast.LENGTH_LONG).show()
            finish()
        }
    }

    private fun startConfiguredProfileIfPresent() {
        val profileJson = RuntimeStateStore(this).profileJson
        if (profileJson != null) {
            startForegroundService(InterceptVpnService.startIntent(this, profileJson))
        }
        finish()
    }

    companion object {
        private const val REQUEST_VPN_PERMISSION = 100
    }
}
