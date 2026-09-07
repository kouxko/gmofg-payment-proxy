package com.interceptproxy.vpn

import android.app.job.JobParameters
import android.app.job.JobService
import android.net.VpnService
import android.util.Log

/** 执行重启后的单次恢复；连续失败保护由 [RuntimeStateStore] 负责。 */
class ResumeVpnJobService : JobService() {
    override fun onStartJob(params: JobParameters?): Boolean {
        val state = RuntimeStateStore(this)
        val activation = state.activation
        if (
            state.autoResumeEnabled &&
            activation != null &&
            VpnService.prepare(this) == null
        ) {
            InterceptVpnServiceControl.recoverSavedActivation(this, activation) { generation ->
                AndroidPlatformCompatibility.startVpnService(
                    this,
                    InterceptVpnService.startIntent(
                        this,
                        activation.profileJson,
                        activation.proxyRuntimeJson,
                        generation,
                    ),
                )
            }.onFailure { error ->
                Log.e(TAG, "保存的 VPN activation 无效，无法恢复", error)
            }
        }
        jobFinished(params, false)
        return false
    }

    override fun onStopJob(params: JobParameters?): Boolean = false

    companion object {
        private const val TAG = "ResumeVpnJobService"
    }
}
