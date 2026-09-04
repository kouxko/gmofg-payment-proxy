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

$CargoConfig = @"
[source.crates-io]
replace-with = "rsproxy"

[source.rsproxy]
registry = "$env:CARGO_RSPROXY_INDEX"
"@
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText(
    (Join-Path $env:CARGO_HOME "config.toml"),
    $CargoConfig,
    $Utf8NoBom
)

$DenoExecutable = Join-Path $env:DENO_INSTALL "deno.exe"
$DenoArchive = Join-Path $ToolsRoot "deno-$env:DENO_VERSION-windows-x64.zip"
function Get-DenoVersion([string]$Executable) {
    $VersionLine = & $Executable --version | Select-Object -First 1
    if ($VersionLine -match '^deno\s+(\S+)') {
        return $Matches[1]
    }
    return $null
}

function Find-VcVars64 {
    $Candidates = @()
    if (-not [string]::IsNullOrWhiteSpace($env:VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH)) {
        $ConfiguredVcVars = Join-Path `
            $env:VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH `
            "VC/Auxiliary/Build/vcvars64.bat"
        if (Test-Path $ConfiguredVcVars -PathType Leaf) {
            $Candidates += Get-Item $ConfiguredVcVars
        }
    }

    foreach ($ProgramFilesRoot in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if (-not [string]::IsNullOrWhiteSpace($ProgramFilesRoot)) {
            $Candidates += Get-ChildItem `
                -Path (Join-Path $ProgramFilesRoot "Microsoft Visual Studio/*/*/VC/Auxiliary/Build/vcvars64.bat") `
                -File `
                -ErrorAction SilentlyContinue
        }
    }

    return $Candidates |
        Sort-Object FullName -Descending -Unique |
        Select-Object -First 1
}

function Import-VcVars64([System.IO.FileInfo]$VcVars) {
    $EnvironmentLines = & cmd.exe /d /s /c "`"$($VcVars.FullName)`" >nul && set"
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to load the MSVC environment from $($VcVars.FullName)."
    }
    foreach ($EnvironmentLine in $EnvironmentLines) {
        $Separator = $EnvironmentLine.IndexOf('=')
        if ($Separator -gt 0) {
            $Name = $EnvironmentLine.Substring(0, $Separator)
            $Value = $EnvironmentLine.Substring($Separator + 1)
            Set-Item -Path "Env:$Name" -Value $Value
        }
    }
}
$InstalledDenoVersion = if (Test-Path $DenoExecutable -PathType Leaf) {
    Get-DenoVersion $DenoExecutable
} else {
    $null
}
if ($InstalledDenoVersion -ne $env:DENO_VERSION) {
    Invoke-WebRequest `
        -UseBasicParsing `
        -Uri "$env:DENO_DIST_BASE_URL/v$env:DENO_VERSION/deno-x86_64-pc-windows-msvc.zip" `
        -OutFile $DenoArchive
    Expand-Archive -Path $DenoArchive -DestinationPath $env:DENO_INSTALL -Force
}

$CargoBin = Join-Path $env:CARGO_HOME "bin"
$RustupExecutable = Join-Path $CargoBin "rustup.exe"
$RustupInstaller = Join-Path $ToolsRoot "rustup-init-$env:RUSTUP_VERSION-windows-x64.exe"
if (-not (Test-Path $RustupExecutable -PathType Leaf)) {
    Invoke-WebRequest `
        -UseBasicParsing `
        -Uri "$env:RUSTUP_UPDATE_ROOT/archive/$env:RUSTUP_VERSION/x86_64-pc-windows-msvc/rustup-init.exe" `
        -OutFile $RustupInstaller
    & $RustupInstaller -y --no-modify-path --profile minimal --default-toolchain none
    if ($LASTEXITCODE -ne 0) {
        throw "rustup installation failed with exit code $LASTEXITCODE."
    }
}

$env:Path = "$env:DENO_INSTALL;$CargoBin;$env:Path"
$InstalledDenoVersion = Get-DenoVersion $DenoExecutable
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
    $Cl = Get-Command cl.exe -ErrorAction SilentlyContinue
    $Link = Get-Command link.exe -ErrorAction SilentlyContinue
    if (-not $Cl -or -not $Link) {
        $VcVars = Find-VcVars64
        if (-not $VcVars) {
            if ([string]::IsNullOrWhiteSpace($env:VISUAL_STUDIO_BUILD_TOOLS_URL)) {
                throw "VISUAL_STUDIO_BUILD_TOOLS_URL is required to install the missing MSVC toolchain."
            }
            if ([string]::IsNullOrWhiteSpace($env:VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH)) {
                throw "VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH is required to install the missing MSVC toolchain."
            }

            $BuildToolsInstaller = Join-Path $ToolsRoot "vs_buildtools.exe"
            Write-Host "Downloading Visual Studio 2022 Build Tools bootstrapper."
            Invoke-WebRequest `
                -UseBasicParsing `
                -Uri $env:VISUAL_STUDIO_BUILD_TOOLS_URL `
                -OutFile $BuildToolsInstaller

            Write-Host "Installing Visual Studio 2022 C++ Build Tools at $env:VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH."
            $BuildToolsProcess = Start-Process `
                -FilePath $BuildToolsInstaller `
                -ArgumentList @(
                    "--installPath", $env:VISUAL_STUDIO_BUILD_TOOLS_INSTALL_PATH,
                    "--add", "Microsoft.VisualStudio.Workload.VCTools",
                    "--includeRecommended",
                    "--quiet",
                    "--wait",
                    "--norestart",
                    "--nocache"
                ) `
                -Wait `
                -PassThru
            if ($BuildToolsProcess.ExitCode -notin @(0, 3010)) {
                throw "Visual Studio Build Tools installation failed with exit code $($BuildToolsProcess.ExitCode)."
            }

            $VcVars = Find-VcVars64
            if (-not $VcVars) {
                throw "Visual Studio Build Tools installation completed, but vcvars64.bat was not found."
            }
        }
        Import-VcVars64 $VcVars
        $Cl = Get-Command cl.exe -ErrorAction SilentlyContinue
        $Link = Get-Command link.exe -ErrorAction SilentlyContinue
    }
    if (-not $Cl -or -not $Link) {
        throw "Visual Studio C++ x64 build tools are required by the Windows GitLab Runner."
    }
    Write-Host "MSVC compiler: $($Cl.Source)"
    Write-Host "MSVC linker: $($Link.Source)"
}

deno --version
rustc --version
cargo --version
