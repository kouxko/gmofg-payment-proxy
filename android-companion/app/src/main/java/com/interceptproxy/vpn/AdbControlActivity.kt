package com.interceptproxy.vpn

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.util.Log

/**
 * system ADB 的最小救援控制入口。
 *
 * Manifest 通过 `android.permission.DUMP` 把调用方限制为 shell/root。正式的
 * `adb forward localabstract:intercept_proxy_vpn` 版本化 JSON 通道由
 * [CompanionControlServer] 提供；这个 Activity 只负责拉起进程、显示系统授权以及兼容
 * 旧版显式救援。桌面端普通 start/apply/stop 不会通过 Activity 旁路控制协议。
 */
class AdbControlActivity : Activity() {
    private var pendingProfileJson: String? = null
    private var pendingProxyRuntimeJson: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        when (intent.getStringExtra(EXTRA_COMMAND)) {
            COMMAND_WAKE_CONTROL_SERVER -> finish()
            COMMAND_CONFIGURE_AND_START -> configureAndStart()
            COMMAND_STOP -> {
                InterceptVpnService.stopFromExternalControl(this, "VPN 已停止。")
                finish()
            }
            else -> {
                Log.e(TAG, "未知或缺少 ADB command")
                finish()
            }
        }
    }

    private fun configureAndStart() {
        val rawJson = intent.getStringExtra(EXTRA_PROFILE_JSON)
        if (rawJson.isNullOrBlank()) {
            Log.e(TAG, "configure_and_start 缺少 profile_json")
            finish()
            return
        }
        val proxyRuntimeJson = intent.getStringExtra(EXTRA_PROXY_RUNTIME_JSON)
        if (proxyRuntimeJson.isNullOrBlank()) {
            Log.e(TAG, "configure_and_start 缺少 proxy_runtime_json")
            finish()
            return
        }
        // 这里只做 JSON 形状检查；shared UID 与全部弱网字段由启动时 Rust 校验。
        runCatching { CompanionProfileParser.parse(rawJson) }.onFailure { error ->
            Log.e(TAG, "Profile JSON 无效", error)
            finish()
            return
        }
        runCatching { ProxyRuntimeParser.parse(rawJson, proxyRuntimeJson) }.onFailure { error ->
            Log.e(TAG, "代理路由运行配置无效", error)
            finish()
            return
        }
        pendingProfileJson = rawJson
        pendingProxyRuntimeJson = proxyRuntimeJson
        val permissionIntent = VpnService.prepare(this)
        if (permissionIntent == null) {
            persistAndStart(rawJson, proxyRuntimeJson)
        } else {
            @Suppress("DEPRECATION")
            startActivityForResult(permissionIntent, REQUEST_VPN_PERMISSION)
        }
    }

    @Deprecated("Android 系统 VPN 授权仍通过 Activity result 返回")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val profile = pendingProfileJson
        val proxyRuntime = pendingProxyRuntimeJson
        if (
            requestCode == REQUEST_VPN_PERMISSION &&
            resultCode == RESULT_OK &&
            profile != null &&
            proxyRuntime != null
        ) {
            persistAndStart(profile, proxyRuntime)
        } else {
            Log.e(TAG, "用户拒绝或取消 VPN 授权")
            finish()
        }
    }

    private fun persistAndStart(rawJson: String, proxyRuntimeJson: String) {
        val profile = CompanionProfileParser.parse(rawJson)
        val runtime = ProxyRuntimeParser.parse(rawJson, proxyRuntimeJson)
        RuntimeStateStore(this).clearRecovery()
        val generation = VpnRuntimeRegistry.startRequested(
            org.json.JSONObject(profile.rawJson).getString("id"),
            runtime,
        )
        ControlLeaseManager.configureUnmanagedRecovery(
            this,
            generation,
            profile.stopVpnOnControlLoss,
        )
        AndroidPlatformCompatibility.startVpnService(
            this,
            InterceptVpnService.startIntent(this, rawJson, proxyRuntimeJson, generation),
        )
        finish()
    }

    companion object {
        private const val TAG = "InterceptAdbControl"
        private const val REQUEST_VPN_PERMISSION = 101
        const val EXTRA_COMMAND = "command"
        const val EXTRA_PROFILE_JSON = "profile_json"
        const val EXTRA_PROXY_RUNTIME_JSON = "proxy_runtime_json"
        const val COMMAND_CONFIGURE_AND_START = "configure_and_start"
        const val COMMAND_STOP = "stop"
        const val COMMAND_WAKE_CONTROL_SERVER = "wake_control_server"
    }

}
