pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "intercept-proxy-android-companion"
include(":app")
// 仅供 Android VPN 架构门禁使用的无界面网络探针。它不会被打进桌面安装包，也不会
// 参与 Companion release；独立 UID 用于证明全丢包时至少两款非目标应用仍可联网。
include(":isolation-probe")
// 与 isolation-probe 共用同一份最小网络探针源码，但使用独立 package/UID。它专门作为
// VpnService 的目标应用，避免用 com.android.shell 误把 ADB 控制流量纳入门禁。
include(":target-probe")
