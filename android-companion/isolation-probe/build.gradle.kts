plugins {
    id("com.android.application")
}

android {
    namespace = "com.interceptproxy.vpn.isolationprobe"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.interceptproxy.vpn.isolationprobe"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
    }
}
