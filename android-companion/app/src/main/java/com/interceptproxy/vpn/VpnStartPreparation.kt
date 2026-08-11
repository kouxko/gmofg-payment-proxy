package com.interceptproxy.vpn

import android.net.VpnService

internal sealed interface VpnStartPreparation {
    data class Ready(
        val profile: CompanionProfile,
        val inventoryJson: String,
        val proxyRuntimeJson: String,
        val runtime: ProxyRuntimeFacts,
    ) : VpnStartPreparation

    data class Rejected(val reason: String) : VpnStartPreparation
}

internal object VpnStartPreparer {
    fun prepare(
        service: InterceptVpnService,
        rawJson: String?,
        proxyRuntimeJson: String?,
    ): VpnStartPreparation {
        if (rawJson.isNullOrBlank()) {
            return VpnStartPreparation.Rejected("没有可启动的 Profile")
        }
        if (proxyRuntimeJson.isNullOrBlank()) {
            return VpnStartPreparation.Rejected("没有可启动的代理路由运行配置")
        }
        if (VpnService.prepare(service) != null) {
            return VpnStartPreparation.Rejected("VPN 授权已失效，需要重新确认")
        }

        val profile = runCatching { CompanionProfileParser.parse(rawJson) }
            .getOrElse {
                return VpnStartPreparation.Rejected("Profile JSON 无效：${it.message}")
            }
        val inventoryJson = PackageInventory.collect(service.packageManager).toInventoryJson()
        val rustError = NativeBridge.validateProfile(profile.rawJson, inventoryJson)
        if (rustError.isNotEmpty()) return VpnStartPreparation.Rejected(rustError)

        val runtime = runCatching { ProxyRuntimeParser.parse(rawJson, proxyRuntimeJson) }
            .getOrElse {
                return VpnStartPreparation.Rejected("代理路由运行配置无效：${it.message}")
            }
        if (runtime.routeCount != profile.expectedProxyRouteCount) {
            return VpnStartPreparation.Rejected(
                "代理路由运行配置不完整：方案需要 ${profile.expectedProxyRouteCount} 条，" +
                    "实际装载 ${runtime.routeCount} 条",
            )
        }
        if (!NativeBridge.isDataPlaneAvailable()) {
            return VpnStartPreparation.Rejected("Rust TUN 数据面尚不可用，已保持系统网络直连")
        }
        return VpnStartPreparation.Ready(
            profile = profile,
            inventoryJson = inventoryJson,
            proxyRuntimeJson = proxyRuntimeJson,
            runtime = runtime,
        )
    }
}
