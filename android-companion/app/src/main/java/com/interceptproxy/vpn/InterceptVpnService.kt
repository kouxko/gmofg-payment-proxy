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
    private var tun: ParcelFileDescriptor? = null
    private var tunConfiguration: TunConfiguration? = null
    private var packageChangeReceiver: TargetPackageChangeReceiver? = null
    /** 只存在于当前 Service 生命周期；USB/LAN 地址绝不写入本地 Profile。 */
    private var activeProxyRuntimeJson: String = EMPTY_PROXY_RUNTIME_JSON

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, createNotification())
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> stopVpn(manual = true)
            ACTION_REVALIDATE -> restartSavedProfile()
            ACTION_START -> startProfile(
                intent.getStringExtra(EXTRA_PROFILE_JSON),
                intent.getStringExtra(EXTRA_PROXY_RUNTIME_JSON) ?: EMPTY_PROXY_RUNTIME_JSON,
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
        unregisterPackageReceiver()
        tun?.close()
        tun = null
        NativeBridge.stop()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = super.onBind(intent)

    private fun startProfile(rawJson: String?, proxyRuntimeJson: String = EMPTY_PROXY_RUNTIME_JSON) {
        if (rawJson.isNullOrBlank()) return failOpen("没有可启动的 Profile")
        if (prepare(this) != null) return failOpen("VPN 授权已失效，需要重新确认")

        val profile = runCatching { CompanionProfileParser.parse(rawJson) }
            .getOrElse { return failOpen("Profile JSON 无效：${it.message}") }
        val inventory = PackageInventory.collect(packageManager)
        val inventoryJson = inventory.toInventoryJson()
        // 包签名、UID、共享 UID 整组选择、最大应用数以及弱网参数只由 Rust 校验。
        // Kotlin 仅提供 PackageManager 的事实快照，避免维护第二套业务规则。
        val rustError = NativeBridge.validateProfile(profile.rawJson, inventoryJson)
        if (rustError.isNotEmpty()) return failOpen(rustError)
        if (!NativeBridge.isDataPlaneAvailable()) {
            return failOpen("Rust TUN 数据面尚不可用，已保持系统网络直连")
        }

        val desiredTunConfiguration = TunConfiguration(
            targetPackages = profile.targetPackages.map(TargetPackage::packageName).toSet(),
            mtu = profile.mtu,
        )
        val currentTun = tun
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
            establishTun(profile) ?: return failOpen("Android 未能建立 TUN")
        }
        // Rust 必须独立持有一个 fd。detachFd 后该副本只能由 Rust 关闭；Java 端仍保留
        // newTun，以便 stop/fail-open 时立即撤销 VPN 路由。
        val nativeTunFd = runCatching {
            ParcelFileDescriptor.dup(newTun.fileDescriptor).detachFd()
        }.getOrElse {
            newTun.close()
            return failOpen("复制 Android TUN 文件描述符失败：${it.message}")
        }
        val started = NativeBridge.start(
            tunFd = nativeTunFd,
            profileJson = profile.rawJson,
            inventoryJson = inventoryJson,
            proxyRuntimeJson = proxyRuntimeJson,
            protector = NativeSocketProtector(this),
        )
        if (!started) {
            if (!canReuseTun) newTun.close()
            return failOpen("Rust 数据面启动失败，TUN 已关闭")
        }

        tun = newTun
        tunConfiguration = desiredTunConfiguration
        activeProxyRuntimeJson = proxyRuntimeJson
        stateStore.profileJson = rawJson
        stateStore.autoResumeEnabled = profile.autoResumeAfterReboot
        stateStore.clearFailures()
        VpnRuntimeRegistry.running(JSONObject(rawJson).getString("id"))
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
            .addDnsServer("1.1.1.1")
            .addDnsServer("2606:4700:4700::1111")

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
        startProfile(stateStore.profileJson, activeProxyRuntimeJson)
    }

    /** JNI 从后台线程通知故障；真正的 Service/TUN 操作统一回到 Android 主线程。 */
    internal fun onNativeDataPlaneFailure(reason: String) {
        Handler(Looper.getMainLooper()).post {
            if (tun != null) failOpen("Rust 数据面异常退出：$reason")
        }
    }

    private fun failOpen(reason: String) {
        Log.e(TAG, reason)
        VpnRuntimeRegistry.faulted(reason)
        stateStore.recordFailure()
        closeCurrentDataPlane()
        unregisterPackageReceiver()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun stopVpn(manual: Boolean) {
        if (manual) stateStore.autoResumeEnabled = false
        closeCurrentDataPlane()
        unregisterPackageReceiver()
        VpnRuntimeRegistry.stopped()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    /**
     * 先关闭 Java 持有的 TUN，让 Android 立即撤销目标 UID 路由并唤醒 Rust 的 TUN
     * 读取；随后再等待原生运行时在有限窗口内退出。这个顺序同时保证 fail-open 和
     * Profile 热切换不会因为旧接口地址残留而失败。
     */
    private fun closeCurrentDataPlane() {
        tun?.close()
        tun = null
        tunConfiguration = null
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
        private const val EMPTY_PROXY_RUNTIME_JSON = "{\"routes\":[]}"
        private const val NOTIFICATION_CHANNEL = "intercept_proxy_vpn"
        private const val NOTIFICATION_ID = 41001

        fun startIntent(
            context: Context,
            profileJson: String,
            proxyRuntimeJson: String = EMPTY_PROXY_RUNTIME_JSON,
        ): Intent =
            Intent(context, InterceptVpnService::class.java)
                .setAction(ACTION_START)
                .putExtra(EXTRA_PROFILE_JSON, profileJson)
                .putExtra(EXTRA_PROXY_RUNTIME_JSON, proxyRuntimeJson)

        fun stopIntent(context: Context): Intent =
            Intent(context, InterceptVpnService::class.java).setAction(ACTION_STOP)
    }
}
