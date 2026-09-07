package com.interceptproxy.vpn

import android.net.VpnService

/**
 * 把目标应用的 DNS 查询固定送进 TUN，而不是依赖设备当前的物理网络。
 *
 * `tun2proxy` 的 Virtual DNS 会拦截所有 53 端口查询并返回 Fake-IP。这个地址只
 * 是 Android `VpnService` 的 DNS 入口，不会作为公网 DNS 访问，也不会离开设备。
 */
internal object VpnLocalDns {
    internal const val ADDRESS = "198.18.0.1"

    fun addTo(builder: VpnService.Builder): VpnService.Builder = builder.apply {
        addDnsServer(ADDRESS)
    }
}
