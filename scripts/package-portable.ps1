param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Executable = Join-Path $ProjectRoot "src-tauri/target/release/gmofg-payment-proxy.exe"
$DistDirectory = Join-Path $ProjectRoot "dist"
$PortableDirectory = Join-Path $DistDirectory "GMO-FG-Payment-Proxy-portable-x64"
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

if (Test-Path $PortableDirectory) {
    Remove-Item $PortableDirectory -Recurse -Force
}
if (Test-Path $Archive) {
    Remove-Item $Archive -Force
}

New-Item $PortableDirectory -ItemType Directory -Force | Out-Null
Copy-Item $Executable (Join-Path $PortableDirectory "GMO-FG-Payment-Proxy.exe")
Copy-Item (Join-Path $ProjectRoot "README.md") (Join-Path $PortableDirectory "README.md")

$PortableNotice = @"
GMO-FG Payment Proxy portable build

- This package does not install or modify system-wide files.
- Microsoft Edge WebView2 Runtime must already be available on the target Windows machine.
- Certificates, settings, and rules are stored in the current user's application data.
- Private keys and passwords are protected with Windows DPAPI current-user scope and cannot be moved to another Windows user.
"@
Set-Content -Path (Join-Path $PortableDirectory "PORTABLE.txt") -Value $PortableNotice -Encoding UTF8

Compress-Archive -Path "$PortableDirectory/*" -DestinationPath $Archive -CompressionLevel Optimal
Write-Output "Created $Archive"
