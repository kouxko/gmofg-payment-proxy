param(
    [switch]$RequireMsvc
)

$ErrorActionPreference = "Stop"

$RepositoryRoot = if ([string]::IsNullOrWhiteSpace($env:CI_PROJECT_DIR)) {
    Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
} else {
    $env:CI_PROJECT_DIR
}
$ToolsRoot = Join-Path $RepositoryRoot ".ci-cache/tools"
$env:CARGO_HOME = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $RepositoryRoot ".ci-cache/cargo" }
$env:RUSTUP_HOME = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $RepositoryRoot ".ci-cache/rustup" }
$env:DENO_INSTALL = if ($env:DENO_INSTALL) { $env:DENO_INSTALL } else { Join-Path $ToolsRoot "deno" }
$env:DENO_DIR = if ($env:DENO_DIR) { $env:DENO_DIR } else { Join-Path $RepositoryRoot ".ci-cache/deno-cache" }

@(
    $ToolsRoot,
    $env:CARGO_HOME,
    $env:RUSTUP_HOME,
    $env:DENO_INSTALL,
    $env:DENO_DIR
) | ForEach-Object {
    New-Item -ItemType Directory -Force -Path $_ | Out-Null
}

$DenoExecutable = Join-Path $env:DENO_INSTALL "deno.exe"
$DenoArchive = Join-Path $ToolsRoot "deno-$env:DENO_VERSION-windows-x64.zip"
$DenoArchiveSha256 = if (Test-Path $DenoArchive -PathType Leaf) {
    (Get-FileHash -Algorithm SHA256 -Path $DenoArchive).Hash
} else {
    $null
}
if ($DenoArchiveSha256 -ne $env:DENO_WINDOWS_SHA256) {
    Invoke-WebRequest `
        -UseBasicParsing `
        -Uri "https://github.com/denoland/deno/releases/download/v$env:DENO_VERSION/deno-x86_64-pc-windows-msvc.zip" `
        -OutFile $DenoArchive
    $DenoArchiveSha256 = (Get-FileHash -Algorithm SHA256 -Path $DenoArchive).Hash
}
if ($DenoArchiveSha256 -ne $env:DENO_WINDOWS_SHA256) {
    throw "Deno archive SHA-256 mismatch: $DenoArchiveSha256"
}
Expand-Archive -Path $DenoArchive -DestinationPath $env:DENO_INSTALL -Force

$CargoBin = Join-Path $env:CARGO_HOME "bin"
$RustupExecutable = Join-Path $CargoBin "rustup.exe"
$RustupInstaller = Join-Path $ToolsRoot "rustup-init-$env:RUSTUP_VERSION-windows-x64.exe"
$RustupInstallerSha256 = if (Test-Path $RustupInstaller -PathType Leaf) {
    (Get-FileHash -Algorithm SHA256 -Path $RustupInstaller).Hash
} else {
    $null
}
if ($RustupInstallerSha256 -ne $env:RUSTUP_WINDOWS_SHA256) {
    Invoke-WebRequest `
        -UseBasicParsing `
        -Uri "https://static.rust-lang.org/rustup/archive/$env:RUSTUP_VERSION/x86_64-pc-windows-msvc/rustup-init.exe" `
        -OutFile $RustupInstaller
    $RustupInstallerSha256 = (Get-FileHash -Algorithm SHA256 -Path $RustupInstaller).Hash
}
if ($RustupInstallerSha256 -ne $env:RUSTUP_WINDOWS_SHA256) {
    throw "rustup-init SHA-256 mismatch: $RustupInstallerSha256"
}
& $RustupInstaller -y --no-modify-path --profile minimal --default-toolchain none
if ($LASTEXITCODE -ne 0) {
    throw "rustup installation failed with exit code $LASTEXITCODE."
}

$env:Path = "$env:DENO_INSTALL;$CargoBin;$env:Path"
$InstalledDenoVersion = (& $DenoExecutable --version | Select-Object -First 1) -replace '^deno ', ''
if ($InstalledDenoVersion -ne $env:DENO_VERSION) {
    throw "Deno executable version mismatch: $InstalledDenoVersion"
}
& $RustupExecutable toolchain install $env:RUST_TOOLCHAIN --profile minimal
if ($LASTEXITCODE -ne 0) { throw "Rust toolchain installation failed." }
& $RustupExecutable default $env:RUST_TOOLCHAIN
if ($LASTEXITCODE -ne 0) { throw "Rust toolchain selection failed." }
& $RustupExecutable component add rustfmt clippy --toolchain $env:RUST_TOOLCHAIN
if ($LASTEXITCODE -ne 0) { throw "Rust component installation failed." }
& $RustupExecutable target add wasm32-wasip2 --toolchain $env:RUST_TOOLCHAIN
if ($LASTEXITCODE -ne 0) { throw "Rust target installation failed." }

if ($RequireMsvc) {
    $VsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
    if (-not (Test-Path $VsWhere -PathType Leaf)) {
        throw "Visual Studio Installer metadata was not found at $VsWhere."
    }
    $Installation = & $VsWhere `
        -latest `
        -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath
    if ([string]::IsNullOrWhiteSpace($Installation)) {
        throw "Visual Studio C++ x64 build tools are required by the Windows GitLab Runner."
    }
}

deno --version
rustc --version
cargo --version
