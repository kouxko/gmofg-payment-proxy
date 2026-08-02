package com.interceptproxy.vpn

import android.content.Context

/** 保存非敏感运行恢复状态；网络 Payload 永不写入磁盘。 */
class RuntimeStateStore(context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    var profileJson: String?
        get() = preferences.getString(KEY_PROFILE, null)
        set(value) = preferences.edit().putString(KEY_PROFILE, value).apply()

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
        private const val PREFERENCES = "intercept_proxy_vpn_runtime"
        private const val KEY_PROFILE = "profile_json"
        private const val KEY_AUTO_RESUME = "auto_resume"
        private const val KEY_FAILURES = "failure_timestamps"
        private const val FAILURE_WINDOW_MILLIS = 5 * 60 * 1_000L
        private const val MAX_FAILURES = 3
    }
}
