package com.interceptproxy.vpn

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.VpnService
import java.net.InetAddress

/** Reads DNS only from the active non-VPN network before a new TUN is established. */
internal object PhysicalNetworkDns {
    fun addTo(builder: VpnService.Builder, context: Context): VpnService.Builder = builder.apply {
        resolve(context).forEach { server -> addDnsServer(server) }
    }

    private fun resolve(context: Context): List<InetAddress> {
        val connectivity = context.getSystemService(ConnectivityManager::class.java)
        val network = checkNotNull(connectivity.activeNetwork) {
            "没有可用的活动物理网络，无法继承 DNS"
        }
        val capabilities = checkNotNull(connectivity.getNetworkCapabilities(network)) {
            "无法读取活动网络能力，无法继承 DNS"
        }
        val isPhysicalNetwork =
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN) &&
                !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
        val linkProperties = checkNotNull(connectivity.getLinkProperties(network)) {
            "无法读取活动物理网络配置，无法继承 DNS"
        }
        return DnsServerSelector.select(isPhysicalNetwork, linkProperties.dnsServers)
    }
}

/** Pure selection policy kept separate from Android services for local JVM tests. */
internal object DnsServerSelector {
    fun select(isPhysicalNetwork: Boolean, candidates: List<InetAddress>): List<InetAddress> {
        check(isPhysicalNetwork) { "活动网络不是物理网络，拒绝从 VPN 继承 DNS" }
        return candidates.distinctBy(InetAddress::getHostAddress).also { servers ->
            check(servers.isNotEmpty()) { "活动物理网络没有提供 DNS，拒绝建立 TUN" }
        }
    }
}
