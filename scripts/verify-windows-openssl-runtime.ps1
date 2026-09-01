$ErrorActionPreference = "Stop"

$app = Get-Item "src-tauri/target/release/intercept-proxy.exe"
$vswhere = Join-Path ${env:ProgramFiles(x86)} `
  "Microsoft Visual Studio/Installer/vswhere.exe"
$installation = & $vswhere `
  -latest `
  -products * `
  -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
  -property installationPath
if ([string]::IsNullOrWhiteSpace($installation)) {
  throw "Visual Studio C++ tools were not found for DLL dependency inspection."
}
$dumpbin = Get-ChildItem `
  (Join-Path $installation "VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe") `
  -File |
  Sort-Object FullName -Descending |
  Select-Object -First 1
if ($null -eq $dumpbin) {
  throw "dumpbin.exe was not found for DLL dependency inspection."
}
$dependencies = & $dumpbin.FullName /DEPENDENTS $app.FullName 2>&1
if ($LASTEXITCODE -ne 0) {
  throw "dumpbin failed while inspecting $($app.FullName)."
}
$forbidden = $dependencies | Select-String `
  -Pattern '(?i)\b(libssl|libcrypto|ssleay32|libeay32)[^\s]*\.dll\b'
if ($forbidden) {
  throw "Windows executable depends on an OpenSSL runtime DLL: $forbidden"
}
