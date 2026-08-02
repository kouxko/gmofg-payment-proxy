import java.util.Properties

plugins {
    id("com.android.application")
}

// Companion 必须始终使用同一签名身份，否则已经安装在设备上的 APK 无法被后续版本覆盖更新。
// 这个仓库内身份只用于“固定升级身份”，不是保密的应用商店发布凭据；安全边界仍由桌面端的
// ADB shell UID 校验、版本化控制协议和 Android VpnService 授权共同保证。
val signingDirectory = rootProject.file("signing")
val signingPropertiesFile = signingDirectory.resolve("signing.properties")
check(signingPropertiesFile.isFile) { "缺少固定 Companion 签名配置：$signingPropertiesFile" }
val signingProperties = Properties().apply {
    signingPropertiesFile.inputStream().use(::load)
}
fun requiredSigningProperty(name: String): String =
    signingProperties.getProperty(name)?.takeIf(String::isNotBlank)
        ?: throw GradleException("固定 Companion 签名配置缺少：$name")
val releaseKeystore = signingDirectory.resolve(requiredSigningProperty("storeFile"))
check(releaseKeystore.isFile) { "缺少固定 Companion keystore：$releaseKeystore" }

android {
    namespace = "com.interceptproxy.vpn"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.interceptproxy.vpn"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters += setOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
        }
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    signingConfigs {
        create("stableCompanion") {
            storeFile = releaseKeystore
            storePassword = requiredSigningProperty("storePassword")
            keyAlias = requiredSigningProperty("keyAlias")
            keyPassword = requiredSigningProperty("keyPassword")
            enableV1Signing = true
            enableV2Signing = true
            enableV3Signing = true
            enableV4Signing = true
        }
    }

    buildTypes {
        getByName("release") {
            signingConfig = signingConfigs.getByName("stableCompanion")
        }
    }
    packaging {
        jniLibs {
            // Rust cdylib 需要按页对齐并以未压缩形式装入 APK，兼容 16 KiB page size 设备。
            useLegacyPackaging = false
        }
    }

    testOptions {
        unitTests.isIncludeAndroidResources = false
    }
}

val buildRustAndroid = tasks.register<Exec>("buildRustAndroid") {
    group = "rust"
    description = "构建四 ABI Rust TUN 数据面并复制到 jniLibs"
    workingDir(rootProject.projectDir)
    commandLine("bash", rootProject.file("scripts/build-rust-android.sh"))
    inputs.files(
        rootProject.file("../src-tauri/crates/android-engine/Cargo.toml"),
        rootProject.fileTree("../src-tauri/crates/android-engine/src") {
            include("**/*.rs")
        },
    )
    outputs.dir(layout.projectDirectory.dir("src/main/jniLibs"))
}

tasks.named("preBuild").configure {
    dependsOn(buildRustAndroid)
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test:runner:1.7.0")
}
