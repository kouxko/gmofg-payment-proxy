package com.interceptproxy.vpn

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent

/** 重启或 Companion 升级后至少等待 30 秒再尝试恢复，不要求设备物理网络可用。 */
class BootCompletedReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        // 即使 Manifest 只声明系统广播，也要再次校验 action，避免导出 Receiver 被伪造调用。
        if (intent.action != Intent.ACTION_BOOT_COMPLETED &&
            intent.action != Intent.ACTION_MY_PACKAGE_REPLACED
        ) {
            return
        }
        val state = RuntimeStateStore(context)
        if (!state.autoResumeEnabled || state.activation == null) return
        val job = JobInfo.Builder(JOB_ID, ComponentName(context, ResumeVpnJobService::class.java))
            .setMinimumLatency(30_000)
            .setPersisted(true)
            .build()
        context.getSystemService(JobScheduler::class.java).schedule(job)
    }

    companion object {
        private const val JOB_ID = 41002
    }
}
