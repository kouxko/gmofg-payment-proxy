package com.interceptproxy.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.VpnService
import android.os.Build
import android.os.IBinder
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.util.Log
import java.lang.ref.WeakReference
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

/**
 * 只接管 Profile 中显式选择应用的 VpnService。
 *
 * 任何校验、JNI 或 TUN 错误都会关闭文件描述符并停止服务。关闭 TUN 后 Android 自动
 * 把目标应用恢复到系统网络，因此这里的失败策略是 fail-open，而不是锁死设备网络。
 */
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
    private var packageChangeReceiver: TargetPackageChangeReceiver? = null

    override fun onCreate() {
        super.onCreate()
        activeService.set(WeakReference(this))
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, createNotification())
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> stopVpn(manual = true)
            ACTION_REVALIDATE -> restartSavedProfile()
            ACTION_START -> startProfile(
                intent.getStringExtra(EXTRA_PROFILE_JSON),
                intent.getStringExtra(EXTRA_PROXY_RUNTIME_JSON),
                intent.getLongExtra(EXTRA_GENERATION, INVALID_GENERATION),
            )
            else -> restartSavedProfile()
        }
        return START_NOT_STICKY
    }

    override fun onRevoke() {
        // 系统撤销授权时备用网络已经恢复，可直接释放 TUN，不需要尝试排空。
        stopVpn(manual = false)
        super.onRevoke()
    }

    override fun onDestroy() {
        activeService.get()?.get()?.let { current ->
            if (current === this) activeService.set(null)
        }
        unregisterPackageReceiver()
        tun.getAndSet(null)?.close()
        NativeBridge.stop()
        activeGeneration = null
        val snapshot = VpnRuntimeRegistry.snapshot()
        if (snapshot.state == "stop_requested") {
            VpnRuntimeRegistry.confirmStopped(snapshot.generation)
        } else if (snapshot.state == "running" || snapshot.state == "start_requested") {
            VpnRuntimeRegistry.faulted("VpnService 已销毁，TUN 已关闭并恢复系统网络。")
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = super.onBind(intent)

    /**
     * 按 generation 启动一代完整数据面：解析输入 -> Rust 领域校验 -> 建立/复用 TUN -> JNI。
     *
     * generation 是桌面端配置事务的防旧写屏障；每个可能耗时的阶段后都重新确认，过期启动
     * 不得覆盖较新的 stop/apply。Kotlin 只负责 Android 权限、包清单与 fd 生命周期，规则和
     * 路由完整性统一交给 Rust。TUN 在进入 JNI 前公开到 AtomicReference，使外部超时清理可以
     * 立即关闭系统路由。只有 JNI 成功且 registry 接受同一 generation 后才发布 running；任一
     * 失败都走 failOpen，关闭 TUN 并让目标应用恢复系统直连。
     */
    private fun startProfile(rawJson: String?, proxyRuntimeJson: String?, generation: Long) {
        if (!continueStart(generation)) return
        if (rawJson.isNullOrBlank()) return failOpen("没有可启动的 Profile", generation)
        if (proxyRuntimeJson.isNullOrBlank()) {
            return failOpen("没有可启动的代理路由运行配置", generation)
        }
        if (prepare(this) != null) return failOpen("VPN 授权已失效，需要重新确认", generation)

        val profile = runCatching { CompanionProfileParser.parse(rawJson) }
            .getOrElse { return failOpen("Profile JSON 无效：${it.message}", generation) }
        val inventory = PackageInventory.collect(packageManager)
        val inventoryJson = inventory.toInventoryJson()
        // UID、共享 UID 整组选择、最大应用数以及弱网参数只由 Rust 校验。
        // Kotlin 仅提供 PackageManager 的事实快照，避免维护第二套业务规则。
        val rustError = NativeBridge.validateProfile(profile.rawJson, inventoryJson)
        if (rustError.isNotEmpty()) return failOpen(rustError, generation)
        val runtime = runCatching { ProxyRuntimeParser.parse(rawJson, proxyRuntimeJson) }
            .getOrElse {
                return failOpen("代理路由运行配置无效：${it.message}", generation)
            }
        if (runtime.routeCount != profile.expectedProxyRouteCount) {
            return failOpen(
                "代理路由运行配置不完整：方案需要 ${profile.expectedProxyRouteCount} 条，实际装载 ${runtime.routeCount} 条",
                generation,
            )
        }
        if (!NativeBridge.isDataPlaneAvailable()) {
            return failOpen("Rust TUN 数据面尚不可用，已保持系统网络直连", generation)
        }
        if (!continueStart(generation)) return

        val desiredTunConfiguration = TunConfiguration(
            targetPackages = profile.targetPackages.map(TargetPackage::packageName).toSet(),
            mtu = profile.mtu,
        )
        val currentTun = tun.get()
        val canReuseTun = currentTun != null && tunConfiguration == desiredTunConfiguration
        val newTun = if (canReuseTun) {
            // 仅修改延迟、丢包、限速等弱网参数时，目标 UID 路由与 TUN MTU 都没有
            // 改变。复用同一个 TUN，只重启 Rust 数据面，既避免不必要的断网，也规避
            // 某些 Android/模拟器连续创建 IPv6 TUN 后 `Cannot set address` 的系统缺陷。
            NativeBridge.stop()
            checkNotNull(currentTun)
        } else {
            // 目标应用或 TUN MTU 改变时必须重建 allowlist。先撤销旧路由，再建立新
            // 接口，不能让两个 TUN 同时争用同一个 IPv6 /128 地址。
            closeCurrentDataPlane()
            establishTun(profile) ?: return failOpen("Android 未能建立 TUN", generation)
        }
        if (!canReuseTun) {
            // 在进入可能阻塞的 JNI 前公开 TUN 所有权。外部 stop 超时线程随后可以
            // getAndSet(null) 并立即撤销 Android 路由，不必等待主线程返回。
            tun.set(newTun)
            tunConfiguration = desiredTunConfiguration
            activeGeneration = generation
        }
        // Rust 必须独立持有一个 fd。detachFd 后该副本只能由 Rust 关闭；Java 端仍保留
        // newTun，以便 stop/fail-open 时立即撤销 VPN 路由。
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
            proxyRuntimeJson = proxyRuntimeJson,
            protector = NativeSocketProtector(this),
        )
        if (!started) {
            return failOpen("Rust 数据面启动失败，TUN 已关闭", generation)
        }
        if (!continueStart(generation)) return

        tunConfiguration = desiredTunConfiguration
        if (!VpnRuntimeRegistry.running(
                JSONObject(rawJson).getString("id"),
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
            stateStore.activation = StoredActivation(rawJson, proxyRuntimeJson)
            stateStore.autoResumeEnabled = profile.autoResumeAfterReboot
        } else {
            // 透明代理运行端点来自本次 ADB reverse/LAN 链路。即使 Profile 请求自动
            // 恢复，也必须 fail-open，等待桌面端重新解析路由并显式 start/apply。
            stateStore.clearRecovery()
        }
        stateStore.clearFailures()
        registerPackageReceiver(profile.targetPackages.map(TargetPackage::packageName).toSet())
    }

    private fun establishTun(profile: CompanionProfile): ParcelFileDescriptor? = runCatching {
        val builder = Builder()
            .setSession(profile.targetPackages.joinToString { it.packageName })
            .setMtu(profile.mtu)
            .addAddress("10.254.0.2", 32)
            .addAddress("fd00:6970:7670::2", 128)
            .addRoute("0.0.0.0", 0)
            .addRoute("::", 0)
            .let { PhysicalNetworkDns.addTo(it, this) }

        for (target in profile.targetPackages) {
            // 非空 allowlist 的语义是：只有这些包进 VPN，系统和其他应用继续直连。
            builder.addAllowedApplication(target.packageName)
        }
        builder.establish()
    }.getOrElse { error ->
        // VpnService.Builder 的系统端异常也必须遵循 fail-open，不允许逃出
        // onStartCommand 触发应用崩溃和系统反复重启 Service。
        Log.e(TAG, "建立 Android TUN 失败", error)
        null
    }

    private fun restartSavedProfile() {
        val activation = stateStore.activation ?: return failOpen("没有可恢复的 VPN activation")
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
        unregisterPackageReceiver()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun stopVpn(
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
                unregisterPackageReceiver()
            },
        )
        if (!completed && !VpnRuntimeRegistry.isStopped(stopRequest.generation)) return
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    /** 后台控制线程等待主线程超时时，立即释放 TUN；Service 生命周期仍由主线程收尾。 */
    private fun releaseTunAfterExternalStopTimeout(
        stopRequest: VpnRuntimeRegistry.StopRequest,
    ): Boolean = VpnExternalStopCoordinator.completeActiveServiceStop(
        stopRequest = stopRequest,
        message = "等待 Android 主线程关闭 VPN 超时；TUN 已由控制线程强制关闭。",
        releaseTun = ::closeCurrentDataPlane,
    )

    /** 忽略过期 start；若 stop 正在等待，则完成真实数据面清理后再确认停止。 */
    private fun continueStart(generation: Long): Boolean {
        if (generation != INVALID_GENERATION && VpnRuntimeRegistry.canStart(generation)) return true
        discardStaleStart(generation)
        return false
    }

    private fun discardStaleStart(generation: Long, ownsNewDataPlane: Boolean = false) {
        val snapshot = VpnRuntimeRegistry.snapshot()
        val stopPending = snapshot.state == "stop_requested"
        if (ownsNewDataPlane || activeGeneration == generation || stopPending) {
            closeCurrentDataPlane()
            unregisterPackageReceiver()
            activeGeneration = null
        }
        if (stopPending) {
            VpnRuntimeRegistry.confirmStopped(snapshot.generation)
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        } else if (snapshot.state == "stopped" && activeGeneration == null) {
            // queued start 可能已被外部 stopService 取消并确认停止，但 Service 与旧 Intent
            // 仍可能在竞态窗口内到达。generation 已失效，此实例不得继续留在前台。
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    /**
     * 先关闭 Java 持有的 TUN，让 Android 立即撤销目标 UID 路由并唤醒 Rust 的 TUN
     * 读取；随后再等待原生运行时在有限窗口内退出。这个顺序同时保证 fail-open 和
     * Profile 热切换不会因为旧接口地址残留而失败。
     */
    private fun closeCurrentDataPlane() {
        tun.getAndSet(null)?.close()
        tunConfiguration = null
        activeGeneration = null
        NativeBridge.stop()
    }

    private fun registerPackageReceiver(targetPackages: Set<String>) {
        unregisterPackageReceiver()
        packageChangeReceiver = TargetPackageChangeReceiver(targetPackages).also { receiver ->
            val filter = IntentFilter().apply {
                addAction(Intent.ACTION_PACKAGE_ADDED)
                addAction(Intent.ACTION_PACKAGE_REMOVED)
                addAction(Intent.ACTION_PACKAGE_REPLACED)
                addDataScheme("package")
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                registerReceiver(receiver, filter, RECEIVER_EXPORTED)
            } else {
                @Suppress("DEPRECATION")
                registerReceiver(receiver, filter)
            }
        }
    }

    private fun unregisterPackageReceiver() {
        packageChangeReceiver?.let { receiver -> runCatching { unregisterReceiver(receiver) } }
        packageChangeReceiver = null
    }

    private fun createNotificationChannel() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                NOTIFICATION_CHANNEL,
                getString(R.string.vpn_notification_channel),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    private fun createNotification(): Notification {
        val stopIntent = PendingIntent.getService(
            this,
            0,
            stopIntent(this),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val openIntent = PendingIntent.getActivity(
            this,
            1,
            Intent(this, VpnConsentActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stopAction = Notification.Action.Builder(
            null,
            getString(R.string.vpn_stop),
            stopIntent,
        ).build()
        return Notification.Builder(this, NOTIFICATION_CHANNEL)
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setContentTitle(getString(R.string.vpn_notification_title))
            .setContentText(getString(R.string.vpn_notification_text))
            .setContentIntent(openIntent)
            .setOngoing(true)
            .addAction(stopAction)
            .build()
    }

    companion object {
        private const val TAG = "InterceptVpnService"
        private const val ACTION_START = "com.interceptproxy.vpn.action.START"
        private const val ACTION_STOP = "com.interceptproxy.vpn.action.STOP"
        internal const val ACTION_REVALIDATE = "com.interceptproxy.vpn.action.REVALIDATE"
        private const val EXTRA_PROFILE_JSON = "profile_json"
        private const val EXTRA_PROXY_RUNTIME_JSON = "proxy_runtime_json"
        private const val EXTRA_GENERATION = "generation"
        private const val INVALID_GENERATION = -1L
        private const val NOTIFICATION_CHANNEL = "intercept_proxy_vpn"
        private const val NOTIFICATION_ID = 41001
        private const val EXTERNAL_STOP_TIMEOUT_SECONDS = 3L
        private val activeService = AtomicReference<WeakReference<InterceptVpnService>?>(null)

        fun startIntent(
            context: Context,
            profileJson: String,
            proxyRuntimeJson: String,
            generation: Long,
        ): Intent =
            Intent(context, InterceptVpnService::class.java)
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

        /**
         * 桌面控制 socket 和受 DUMP 权限保护的救援 Activity 可能在应用后台运行。
         * 这类调用不能用 `startService(ACTION_STOP)`，否则旧版 Android 会以后台服务
         * 限制拒绝请求。直接通知当前 Service 实例关闭 TUN；只有实例已经不存在时才
         * 使用 `stopService` 清理残留启动状态。
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
}
