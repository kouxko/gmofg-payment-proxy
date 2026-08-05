package com.interceptproxy.vpn

import android.content.Context
import org.json.JSONObject

/** 一次启动所需的完整非敏感配置；两个 JSON 必须作为一个整体读写。 */
data class StoredActivation(
    val profileJson: String,
    val proxyRuntimeJson: String,
)

/** 保存非敏感运行恢复状态；网络 Payload 永不写入磁盘。 */
class RuntimeStateStore(context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    /**
     * Profile 与代理路由必须位于同一条 SharedPreferences 记录中。
     *
     * 单独保存两个 key 会允许进程在两次写入之间退出，恢复时把新 Profile 与旧路由
     * 拼在一起。一个 JSON 字符串由 SharedPreferences 原子替换，可消除这种撕裂状态。
     */
    var activation: StoredActivation?
        get() {
            val raw = preferences.getString(KEY_ACTIVATION, null) ?: return null
            return runCatching {
                val root = JSONObject(raw)
                StoredActivation(
                    profileJson = root.getJSONObject("profile").toString(),
                    proxyRuntimeJson = root.getJSONObject("proxy_runtime").toString(),
                ).also { stored ->
                    val profile = CompanionProfileParser.parse(stored.profileJson)
                    val runtime = ProxyRuntimeParser.parse(
                        stored.profileJson,
                        stored.proxyRuntimeJson,
                    )
                    require(profile.expectedProxyRouteCount == 0 && runtime.routeCount == 0) {
                        "临时代理路由 activation 不可自动恢复"
                    }
                }
            }.getOrElse {
                preferences.edit()
                    .remove(KEY_ACTIVATION)
                    .putBoolean(KEY_AUTO_RESUME, false)
                    .commit()
                null
            }
        }
        set(value) {
            val editor = preferences.edit()
            if (value == null) {
                editor.remove(KEY_ACTIVATION).commit()
            } else {
                val profile = CompanionProfileParser.parse(value.profileJson)
                val runtime = ProxyRuntimeParser.parse(value.profileJson, value.proxyRuntimeJson)
                if (profile.expectedProxyRouteCount != 0 || runtime.routeCount != 0) {
                    // ADB reverse 端口和本次桌面端点只在当前连接期间有效。把它们写入
                    // 自动恢复状态会让设备重启后连接一个已经失效或属于其他进程的端点。
                    check(
                        editor
                            .remove(KEY_ACTIVATION)
                            .putBoolean(KEY_AUTO_RESUME, false)
                            .commit(),
                    ) { "无法清理不可恢复的 VPN activation" }
                    return
                }
                val encoded = JSONObject()
                    .put("profile", JSONObject(value.profileJson))
                    .put("proxy_runtime", JSONObject(value.proxyRuntimeJson))
                    .toString()
                check(editor.putString(KEY_ACTIVATION, encoded).commit()) {
                    "无法持久化 VPN activation"
                }
            }
        }

    /** 清除上一次恢复快照；显式 start/apply 必须在启动新数据面前调用。 */
    fun clearRecovery() {
        check(
            preferences.edit()
                .remove(KEY_ACTIVATION)
                .putBoolean(KEY_AUTO_RESUME, false)
                .commit(),
        ) { "无法清理 VPN 自动恢复状态" }
    }

    var autoResumeEnabled: Boolean
        get() = preferences.getBoolean(KEY_AUTO_RESUME, false)
        set(value) = preferences.edit().putBoolean(KEY_AUTO_RESUME, value).apply()

    fun recordFailure(nowMillis: Long = System.currentTimeMillis()): Boolean {
        val recent = preferences.getString(KEY_FAILURES, "")
            .orEmpty()
            .split(',')
            .mapNotNull(String::toLongOrNull)
            .filter { timestamp -> nowMillis - timestamp <= FAILURE_WINDOW_MILLIS }
            .plus(nowMillis)
        preferences.edit().putString(KEY_FAILURES, recent.joinToString(",")).apply()
        if (recent.size >= MAX_FAILURES) {
            autoResumeEnabled = false
            return false
        }
        return true
    }

    fun clearFailures() {
        preferences.edit().remove(KEY_FAILURES).apply()
    }

    companion object {
        internal const val PREFERENCES = "intercept_proxy_vpn_runtime"
        internal const val KEY_ACTIVATION = "activation_json"
        private const val KEY_AUTO_RESUME = "auto_resume"
        private const val KEY_FAILURES = "failure_timestamps"
        private const val FAILURE_WINDOW_MILLIS = 5 * 60 * 1_000L
        private const val MAX_FAILURES = 3
    }
}
