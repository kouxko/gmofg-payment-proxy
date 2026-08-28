package com.interceptproxy.vpn

import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.IBinder
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.util.Log
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

/** 只接管 Profile 显式选择的应用；任何启动失败都关闭 TUN 并恢复系统直连。 */
class InterceptVpnService : VpnService() {
    private data class TunConfiguration(
        val targetPackages: Set<String>,
        val mtu: Int,
    )
    private val stateStore by lazy { RuntimeStateStore(this) }
    /**
     * TUN 所有权不能依赖 Service 实例锁。外部控制线程在主线程/JNI 卡住时，必须仍能
     * 原子取得并关闭 fd，才能兑现三秒超时后的 fail-open 契约。
     */
    private val tun = AtomicReference<ParcelFileDescriptor?>(null)
    @Volatile private var tunConfiguration: TunConfiguration? = null
    @Volatile private var activeGeneration: Long? = null
    private val packageReceiverRegistration by lazy {
        TargetPackageReceiverRegistration(this)
    }

    override fun onCreate() {
        super.onCreate()
        VpnServiceResourceOrder.startService(
            attachExternalControl = { InterceptVpnServiceControl.attach(this) },
            startForeground = { VpnForegroundNotification.start(this) },
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            InterceptVpnServiceControl.ACTION_STOP -> stopVpn(manual = true)
            ACTION_REVALIDATE -> restartSavedProfile()
            InterceptVpnServiceControl.ACTION_START -> startProfile(
                intent.getStringExtra(InterceptVpnServiceControl.EXTRA_PROFILE_JSON),
                intent.getStringExtra(InterceptVpnServiceControl.EXTRA_PROXY_RUNTIME_JSON),
                intent.getLongExtra(
                    InterceptVpnServiceControl.EXTRA_GENERATION,
                    InterceptVpnServiceControl.INVALID_GENERATION,
                ),
            )
            else -> restartSavedProfile()
        }
        return START_NOT_STICKY
    }

    override fun onRevoke() {
        // 系统撤销授权时备用网络已经恢复，可直接释放 TUN，不需要尝试排空。
        VpnServiceResourceOrder.revokeService(
            stopVpn = { stopVpn(manual = false) },
            revokeSystemPermission = { super.onRevoke() },
        )
    }

    override fun onDestroy() {
        InterceptVpnServiceControl.detach(this)
        VpnServiceResourceOrder.destroyService(
            unregisterPackageReceiver = packageReceiverRegistration::unregister,
            releaseTun = { tun.getAndSet(null)?.close() },
            stopNativeDataPlane = NativeBridge::stop,
            clearActiveGeneration = { activeGeneration = null },
        )
        val snapshot = VpnRuntimeRegistry.snapshot()
        if (snapshot.state == "stop_requested") {
            VpnRuntimeRegistry.confirmStopped(snapshot.generation)
        } else if (snapshot.state == "running" || snapshot.state == "start_requested") {
            VpnRuntimeRegistry.faulted("VpnService 已销毁，TUN 已关闭并恢复系统网络。")
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = super.onBind(intent)

    /** generation 在耗时阶段前后阻止旧 start 覆盖较新的 stop/apply。 */
    private fun startProfile(rawJson: String?, proxyRuntimeJson: String?, generation: Long) {
        if (!continueStart(generation)) return
        val prepared = when (
            val preparation = VpnStartPreparer.prepare(this, rawJson, proxyRuntimeJson)
        ) {
            is VpnStartPreparation.Ready -> preparation
            is VpnStartPreparation.Rejected -> {
                return failOpen(preparation.reason, generation)
            }
        }
        if (!continueStart(generation)) return
        val profile = prepared.profile
        val inventoryJson = prepared.inventoryJson
        val runtime = prepared.runtime

        val desiredTunConfiguration = TunConfiguration(
            targetPackages = profile.targetPackages.map(TargetPackage::packageName).toSet(),
            mtu = profile.mtu,
        )
        val currentTun = tun.get()
        val canReuseTun = currentTun != null && tunConfiguration == desiredTunConfiguration
        val newTun = if (canReuseTun) {
            // 路由和 MTU 未变时复用 TUN，避免热更新弱网参数造成不必要的断网。
            NativeBridge.stop()
            checkNotNull(currentTun)
        } else {
            // allowlist 或 MTU 改变时，先撤销旧路由再建立新接口。
            closeCurrentDataPlane()
            VpnTunFactory.establish(Builder(), profile)
                ?: return failOpen("Android 未能建立 TUN", generation)
        }
        if (!canReuseTun) {
            // JNI 前公开 TUN 所有权，让外部 stop 超时线程可以立即撤销路由。
            tun.set(newTun)
            tunConfiguration = desiredTunConfiguration
            activeGeneration = generation
        }
        // Rust 独占 dup 的 fd；Java 继续持有原 TUN，供 stop/fail-open 撤销路由。
        val nativeTunFd = runCatching {
            ParcelFileDescriptor.dup(newTun.fileDescriptor).detachFd()
        }.getOrElse {
            closeCurrentDataPlane()
            return failOpen("复制 Android TUN 文件描述符失败：${it.message}", generation)
        }
        val started = NativeBridge.start(
            tunFd = nativeTunFd,
            profileJson = profile.rawJson,
            inventoryJson = inventoryJson,
            proxyRuntimeJson = prepared.proxyRuntimeJson,
            protector = NativeSocketProtector(this),
        )
        if (!started) {
            return failOpen("Rust 数据面启动失败，TUN 已关闭", generation)
        }
        if (!continueStart(generation)) return

        tunConfiguration = desiredTunConfiguration
        if (!VpnRuntimeRegistry.running(
                JSONObject(profile.rawJson).getString("id"),
                runtime,
                generation,
            )
        ) {
            discardStaleStart(generation, ownsNewDataPlane = true)
            return
        }
        activeGeneration = generation
        val recoverable = runtime.routeCount == 0 && profile.expectedProxyRouteCount == 0
        if (recoverable) {
            stateStore.activation = StoredActivation(profile.rawJson, prepared.proxyRuntimeJson)
            stateStore.autoResumeEnabled = profile.autoResumeAfterReboot
        } else {
            // 透明代理运行端点来自本次 ADB reverse/LAN 链路。即使 Profile 请求自动
            // 恢复，也必须 fail-open，等待桌面端重新解析路由并显式 start/apply。
            stateStore.clearRecovery()
        }
        stateStore.clearFailures()
        packageReceiverRegistration.register(
            profile.targetPackages.map(TargetPackage::packageName).toSet(),
        )
    }

    private fun restartSavedProfile() {
        InterceptVpnServiceControl.restartSavedProfile(
            context = this,
            stateStore = stateStore,
            failOpen = { reason -> failOpen(reason) },
            startProfile = ::startProfile,
        )
    }

    /** JNI 从后台线程通知故障；真正的 Service/TUN 操作统一回到 Android 主线程。 */
    internal fun onNativeDataPlaneFailure(reason: String) {
        Handler(Looper.getMainLooper()).post {
            if (tun.get() != null) failOpen("Rust 数据面异常退出：$reason")
        }
    }

    private fun failOpen(reason: String, generation: Long? = null) {
        Log.e(TAG, reason)
        if (generation == null) {
            VpnRuntimeRegistry.faulted(reason)
        } else if (!VpnRuntimeRegistry.faultedIfCurrent(generation, reason)) {
            discardStaleStart(generation)
            return
        }
        stateStore.recordFailure()
        closeCurrentDataPlane()
        packageReceiverRegistration.unregister()
        finishStopping()
    }

    internal fun stopVpn(
        manual: Boolean,
        message: String = "VPN 已停止。",
        stopGeneration: Long? = null,
    ) {
        if (manual) stateStore.autoResumeEnabled = false
        val stopRequest = stopGeneration?.let {
            VpnRuntimeRegistry.StopRequest(it, requiresTeardownConfirmation = true)
        } ?: VpnRuntimeRegistry.stopRequested()
        val completed = VpnExternalStopCoordinator.completeActiveServiceStop(
            stopRequest = stopRequest,
            message = message,
            releaseTun = {
                closeCurrentDataPlane()
                packageReceiverRegistration.unregister()
            },
        )
        if (!completed && !VpnRuntimeRegistry.isStopped(stopRequest.generation)) return
        finishStopping()
    }

    /** 后台控制线程等待主线程超时时，立即释放 TUN；Service 生命周期仍由主线程收尾。 */
    internal fun releaseTunAfterExternalStopTimeout(
        stopRequest: VpnRuntimeRegistry.StopRequest,
    ): Boolean = VpnExternalStopCoordinator.completeActiveServiceStop(
        stopRequest = stopRequest,
        message = "等待 Android 主线程关闭 VPN 超时；TUN 已由控制线程强制关闭。",
        releaseTun = ::closeCurrentDataPlane,
    )

    /** 忽略过期 start；若 stop 正在等待，则完成真实数据面清理后再确认停止。 */
    private fun continueStart(generation: Long): Boolean {
        if (
            generation != InterceptVpnServiceControl.INVALID_GENERATION &&
            VpnRuntimeRegistry.canStart(generation)
        ) {
            return true
        }
        discardStaleStart(generation)
        return false
    }

    private fun discardStaleStart(generation: Long, ownsNewDataPlane: Boolean = false) {
        val snapshot = VpnRuntimeRegistry.snapshot()
        val stopPending = snapshot.state == "stop_requested"
        if (ownsNewDataPlane || activeGeneration == generation || stopPending) {
            closeCurrentDataPlane()
            packageReceiverRegistration.unregister()
            activeGeneration = null
        }
        if (stopPending) {
            VpnRuntimeRegistry.confirmStopped(snapshot.generation)
            finishStopping()
        } else if (snapshot.state == "stopped" && activeGeneration == null) {
            // queued start 可能已被外部 stopService 取消并确认停止，但 Service 与旧 Intent
            // 仍可能在竞态窗口内到达。generation 已失效，此实例不得继续留在前台。
            finishStopping()
        }
    }

    /** 先关闭 Java TUN 撤销路由，再停止原生运行时。 */
    private fun closeCurrentDataPlane() {
        VpnServiceResourceOrder.closeCurrentDataPlane(
            releaseTun = { tun.getAndSet(null)?.close() },
            clearTunConfiguration = { tunConfiguration = null },
            clearActiveGeneration = { activeGeneration = null },
            stopNativeDataPlane = NativeBridge::stop,
        )
    }

    private fun finishStopping() {
        VpnServiceResourceOrder.finishStopping(
            stopForeground = { stopForeground(STOP_FOREGROUND_REMOVE) },
            stopService = ::stopSelf,
        )
    }

    companion object {
        private const val TAG = "InterceptVpnService"
        internal const val ACTION_REVALIDATE = InterceptVpnServiceControl.ACTION_REVALIDATE

        fun startIntent(
            context: Context,
            profileJson: String,
            proxyRuntimeJson: String,
            generation: Long,
        ): Intent = InterceptVpnServiceControl.startIntent(
            context,
            profileJson,
            proxyRuntimeJson,
            generation,
        )

        fun startActivationIntent(context: Context, activation: StoredActivation): Intent =
            InterceptVpnServiceControl.startActivationIntent(context, activation)

        fun stopIntent(context: Context): Intent =
            InterceptVpnServiceControl.stopIntent(context)

        fun stopFromExternalControl(context: Context, message: String) {
            InterceptVpnServiceControl.stopFromExternalControl(context, message)
        }
    }
}
