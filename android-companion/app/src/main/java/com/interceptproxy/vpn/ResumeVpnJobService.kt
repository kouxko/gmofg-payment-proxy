package com.interceptproxy.vpn

import android.app.job.JobParameters
import android.app.job.JobService
import android.net.VpnService

/** 执行重启后的单次恢复；连续失败保护由 [RuntimeStateStore] 负责。 */
class ResumeVpnJobService : JobService() {
    override fun onStartJob(params: JobParameters?): Boolean {
        val state = RuntimeStateStore(this)
        val profile = state.profileJson
        if (state.autoResumeEnabled && profile != null && VpnService.prepare(this) == null) {
            startForegroundService(InterceptVpnService.startIntent(this, profile))
        }
        jobFinished(params, false)
        return false
    }

    override fun onStopJob(params: JobParameters?): Boolean = false
}
