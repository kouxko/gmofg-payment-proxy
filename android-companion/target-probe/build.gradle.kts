plugins {
    id("com.android.application")
}

android {
    namespace = "com.interceptproxy.vpn.isolationprobe"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.interceptproxy.vpn.targetprobe"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
    }

    sourceSets.named("main") {
        // 目标探针和隔离探针故意复用同一份极小网络客户端，避免两套门禁实现
        // 随时间产生行为差异；模块自身只改变 applicationId，使它拥有独立 UID。
        java.directories.add("../isolation-probe/src/main/java")
    }
}
