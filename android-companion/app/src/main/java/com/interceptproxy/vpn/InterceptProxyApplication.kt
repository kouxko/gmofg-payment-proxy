package com.interceptproxy.vpn

import android.app.Application

/** 让控制 socket 与 Activity/VpnService 生命周期解耦，只要进程存在就能查询或停止。 */
class InterceptProxyApplication : Application() {
    private lateinit var controlServer: CompanionControlServer

    override fun onCreate() {
        super.onCreate()
        controlServer = CompanionControlServer(this)
        controlServer.start()
    }
}
