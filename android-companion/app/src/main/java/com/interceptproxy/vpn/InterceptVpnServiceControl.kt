package com.interceptproxy.vpn

import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import java.lang.ref.WeakReference
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

internal object InterceptVpnServiceControl {
    const val ACTION_REVALIDATE = "com.interceptproxy.vpn.action.REVALIDATE"
    const val ACTION_START = "com.interceptproxy.vpn.action.START"
    const val ACTION_STOP = "com.interceptproxy.vpn.action.STOP"
    const val EXTRA_PROFILE_JSON = "profile_json"
    const val EXTRA_PROXY_RUNTIME_JSON = "proxy_runtime_json"
    const val EXTRA_GENERATION = "generation"
    const val INVALID_GENERATION = -1L
    private const val EXTERNAL_STOP_TIMEOUT_SECONDS = 3L
    private val activeService = AtomicReference<WeakReference<InterceptVpnService>?>(null)

    fun attach(service: InterceptVpnService) {
        activeService.set(WeakReference(service))
    }

    fun detach(service: InterceptVpnService) {
        activeService.get()?.get()?.let { current ->
            if (current === service) activeService.set(null)
        }
    }

    fun startIntent(
        context: Context,
        profileJson: String,
        proxyRuntimeJson: String,
        generation: Long,
    ): Intent = Intent(context, InterceptVpnService::class.java)
        .setAction(ACTION_START)
        .putExtra(EXTRA_PROFILE_JSON, profileJson)
        .putExtra(EXTRA_PROXY_RUNTIME_JSON, proxyRuntimeJson)
        .putExtra(EXTRA_GENERATION, generation)

    fun startActivationIntent(context: Context, activation: StoredActivation): Intent {
        val profile = CompanionProfileParser.parse(activation.profileJson)
        val runtime = ProxyRuntimeParser.parse(
            activation.profileJson,
            activation.proxyRuntimeJson,
        )
        val generation = VpnRuntimeRegistry.startRequested(
            JSONObject(profile.rawJson).getString("id"),
            runtime,
        )
        return startIntent(
            context,
            activation.profileJson,
            activation.proxyRuntimeJson,
            generation,
        )
    }

    fun stopIntent(context: Context): Intent =
        Intent(context, InterceptVpnService::class.java).setAction(ACTION_STOP)

    fun restartSavedProfile(
        stateStore: RuntimeStateStore,
        failOpen: (String) -> Unit,
        startProfile: (String, String, Long) -> Unit,
    ) {
        val activation = stateStore.activation
            ?: return failOpen("没有可恢复的 VPN activation")
        val profile = runCatching { CompanionProfileParser.parse(activation.profileJson) }
            .getOrElse { return failOpen("Profile JSON 无效：${it.message}") }
        val runtime = runCatching {
            ProxyRuntimeParser.parse(activation.profileJson, activation.proxyRuntimeJson)
        }.getOrElse { return failOpen("代理路由运行配置无效：${it.message}") }
        val generation = VpnRuntimeRegistry.startRequested(
            JSONObject(profile.rawJson).getString("id"),
            runtime,
        )
        startProfile(activation.profileJson, activation.proxyRuntimeJson, generation)
    }

    /**
     * 后台控制 socket 不能用 startService(ACTION_STOP)。直接通知活动实例；实例不存在时
     * 再停止排队中的 Service。后台线程等待主线程三秒，超时后直接释放 TUN。
     */
    fun stopFromExternalControl(context: Context, message: String) {
        RuntimeStateStore(context).autoResumeEnabled = false
        val stopRequest = VpnRuntimeRegistry.stopRequested()
        val service = activeService.get()?.get()
        if (service == null) {
            VpnExternalStopCoordinator.completeWithoutActiveService(
                stopRequest = stopRequest,
                message = message,
                stopNativeDataPlane = NativeBridge::stop,
                stopQueuedService = {
                    context.stopService(Intent(context, InterceptVpnService::class.java))
                    Unit
                },
            )
            return
        }

        if (Looper.myLooper() == Looper.getMainLooper()) {
            service.stopVpn(
                manual = true,
                message = message,
                stopGeneration = stopRequest.generation,
            )
            return
        }

        val completed = CountDownLatch(1)
        Handler(Looper.getMainLooper()).post {
            try {
                service.stopVpn(
                    manual = true,
                    message = message,
                    stopGeneration = stopRequest.generation,
                )
            } finally {
                completed.countDown()
            }
        }
        if (!completed.await(EXTERNAL_STOP_TIMEOUT_SECONDS, TimeUnit.SECONDS)) {
            service.releaseTunAfterExternalStopTimeout(stopRequest)
        }
    }
}
