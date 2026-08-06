param(
    [switch]$SkipBuild
)

# 便携版只打包已经生成的主程序和说明文件，不执行安装，也不写系统目录。
# CI 在完成 MSI/NSIS 构建后使用 -SkipBuild，避免重复编译；开发者本地省略该参数时会先构建。
$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Executable = Join-Path $ProjectRoot "src-tauri/target/release/intercept-proxy.exe"
$CompanionApk = Join-Path $ProjectRoot "src-tauri/resources/android-companion.apk"
$DistDirectory = Join-Path $ProjectRoot "dist"
$PortableDirectory = Join-Path $DistDirectory "Intercept-Proxy-portable-x64"
$Archive = "$PortableDirectory.zip"

if (-not $SkipBuild) {
    Push-Location $ProjectRoot
    try {
        pnpm tauri build --no-bundle
    }
    finally {
        Pop-Location
    }
}

if (-not (Test-Path $Executable -PathType Leaf)) {
    throw "Portable executable was not produced: $Executable"
}
if (-not (Test-Path $CompanionApk -PathType Leaf)) {
    throw "Portable package requires the staged Android Companion APK: $CompanionApk"
}

if (Test-Path $PortableDirectory) {
    # 删除范围被限制在仓库 dist 下的固定目录和同名 zip，不能把变量改成用户目录或磁盘根目录。
    Remove-Item $PortableDirectory -Recurse -Force
}
if (Test-Path $Archive) {
    Remove-Item $Archive -Force
}

New-Item $PortableDirectory -ItemType Directory -Force | Out-Null
Copy-Item $Executable (Join-Path $PortableDirectory "Intercept-Proxy.exe")
Copy-Item (Join-Path $ProjectRoot "README.md") (Join-Path $PortableDirectory "README.md")
$PortableResources = Join-Path $PortableDirectory "resources"
New-Item $PortableResources -ItemType Directory -Force | Out-Null
$PackagedCompanionApk = Join-Path $PortableResources "android-companion.apk"
Copy-Item $CompanionApk $PackagedCompanionApk
# 便携包发布必须 fail-closed：不仅要求目标文件存在，还要确认复制后的字节数一致。
# 这样 CI 不会上传缺少或截断 Companion APK 的 ZIP。
if (-not (Test-Path $PackagedCompanionApk -PathType Leaf) -or
    (Get-Item $PackagedCompanionApk).Length -ne (Get-Item $CompanionApk).Length) {
    throw "Portable package did not contain an intact Android Companion APK."
}

$PortableNotice = @"
Intercept Proxy portable build

- This package does not install or modify system-wide files.
- Microsoft Edge WebView2 Runtime must already be available on the target Windows machine.
- The bundled resources/android-companion.apk is installed on a selected Android device only when requested in the UI.
- Certificates, settings, and rules are stored in the current user's application data.
- Private keys and passwords are protected with Windows DPAPI current-user scope and cannot be moved to another Windows user.
"@
# 便携只表示“无需安装”，不表示数据也能随 U 盘跨机器/跨用户复制。
# 私钥和密码仍由当前 Windows 用户范围的 DPAPI 保护，这是安全设计而不是功能缺失。
Set-Content -Path (Join-Path $PortableDirectory "PORTABLE.txt") -Value $PortableNotice -Encoding UTF8

Compress-Archive -Path "$PortableDirectory/*" -DestinationPath $Archive -CompressionLevel Optimal
Write-Output "Created $Archive"
