param(
    [Parameter(Mandatory = $true)]
    [string]$SourceApk
)

$ErrorActionPreference = "Stop"
$RepositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ResolvedSource = Get-Item $SourceApk -ErrorAction Stop
$DestinationDirectory = Join-Path $RepositoryRoot "src-tauri/resources"
$DestinationApk = Join-Path $DestinationDirectory "android-companion.apk"

New-Item -ItemType Directory -Force -Path $DestinationDirectory | Out-Null
Copy-Item -Path $ResolvedSource.FullName -Destination $DestinationApk -Force

$ResolvedDestination = Get-Item $DestinationApk -ErrorAction Stop
if ($ResolvedDestination.Length -ne $ResolvedSource.Length) {
    throw "The staged Android Companion APK length does not match the verified artifact."
}

Write-Output $ResolvedDestination.FullName
