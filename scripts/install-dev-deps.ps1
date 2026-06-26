# Install workstation dependencies for building and testing ir on Windows.
# This installs system tools only; the first test run still owns the ir/R
# package cache warm-up.

[CmdletBinding()]
param(
    [switch]$DryRun,
    [string[]]$Skip = @()
)

$ErrorActionPreference = "Stop"

$TestRSpec = "oldrel/2"
$TestRName = $null
$TestRVersion = $null
$TestRExcludeNewer = $null
$TestRscript = $null
$RustupInitUrl = "https://win.rustup.rs"
$RigLatestReleaseApi = "https://api.github.com/repos/r-lib/rig/releases/latest"
$SkipRust = $false
$SkipPython = $false
$SkipQuarto = $false
$SkipRRelease = $false
$SkipTestR = $false

foreach ($component in $Skip) {
    switch ($component) {
        "rust" { $SkipRust = $true }
        "python" { $SkipPython = $true }
        "quarto" { $SkipQuarto = $true }
        "r-release" { $SkipRRelease = $true }
        "test-r" { $SkipTestR = $true }
        default { throw "unsupported skip component: $component" }
    }
}

function Write-Step {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    Write-Host ("+ " + $File + " " + ($Arguments -join " "))
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    Write-Step $File $Arguments
    if (-not $DryRun) {
        & $File @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$File exited with code $LASTEXITCODE"
        }
    }
}

function Test-Tool {
    param([Parameter(Mandatory = $true)][string]$Name)

    if ($DryRun) {
        return $false
    }

    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

function Test-AnyTool {
    param([Parameter(Mandatory = $true)][string[]]$Names)

    foreach ($name in $Names) {
        if (Test-Tool $name) {
            return $true
        }
    }
    return $false
}

function Test-RunnableTool {
    param([Parameter(Mandatory = $true)][string]$Name)

    if ($DryRun) {
        return $false
    }

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        return $false
    }

    $windowsApps = Join-Path $env:LOCALAPPDATA "Microsoft\WindowsApps"
    if ($command.Source -and $command.Source.StartsWith($windowsApps, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $false
    }

    & $Name --version *> $null
    return $LASTEXITCODE -eq 0
}

function Test-AnyRunnableTool {
    param([Parameter(Mandatory = $true)][string[]]$Names)

    foreach ($name in $Names) {
        if (Test-RunnableTool $name) {
            return $true
        }
    }
    return $false
}

function Get-PythonTool {
    foreach ($name in @("python", "python3")) {
        if (Test-RunnableTool $name) {
            return $name
        }
    }
    return "python"
}

function Require-Tool {
    param([Parameter(Mandatory = $true)][string]$Name)

    if (-not $DryRun -and -not (Test-Tool $Name)) {
        throw "required command not found: $Name"
    }
}

function Set-TestRMetadata {
    if ($SkipTestR) {
        return
    }

    if ($DryRun) {
        $script:TestRName = "<rig-name-for-$TestRSpec>"
        $script:TestRVersion = "<resolved-$TestRSpec-version>"
        $script:TestRExcludeNewer = "<release-date-for-$TestRSpec>"
        $script:TestRscript = "<Rscript-for-$TestRSpec>"
        return
    }

    $metadata = & (Get-PythonTool) "scripts/resolve-test-r.py" $TestRSpec
    if ($LASTEXITCODE -ne 0) {
        throw "scripts/resolve-test-r.py exited with code $LASTEXITCODE"
    }
    $fields = @($metadata)
    if ($fields.Count -ne 4) {
        throw "scripts/resolve-test-r.py returned unexpected output: $metadata"
    }

    $script:TestRName = $fields[0]
    $script:TestRVersion = $fields[1]
    $script:TestRExcludeNewer = $fields[2]
    $script:TestRscript = $fields[3]
}

function Add-PathIfExists {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ((Test-Path $Path) -and (($env:PATH -split [IO.Path]::PathSeparator) -notcontains $Path)) {
        $env:PATH = "$Path$([IO.Path]::PathSeparator)$env:PATH"
    }
}

function Add-KnownInstallPaths {
    Add-PathIfExists (Join-Path $HOME ".cargo\bin")
    Add-PathIfExists (Join-Path $env:LOCALAPPDATA "Microsoft\WindowsApps")
    Add-PathIfExists (Join-Path $env:LOCALAPPDATA "Programs\Quarto\bin")
    Add-PathIfExists (Join-Path $env:ProgramFiles "Quarto\bin")
    Add-PathIfExists (Join-Path $env:ProgramFiles "R\bin")
    Add-PathIfExists (Join-Path $env:ProgramFiles "rig")
    Add-PathIfExists (Join-Path $env:ProgramFiles "rig\bin")
    Add-PathIfExists (Join-Path $env:ProgramFiles "R\rig\bin")
    Add-PathIfExists (Join-Path $env:LOCALAPPDATA "Programs\R\rig\bin")
}

function Install-WingetPackage {
    param([Parameter(Mandatory = $true)][string]$Id)

    Require-Tool "winget"
    Invoke-Step "winget" @(
        "install",
        "--id",
        $Id,
        "--exact",
        "--accept-package-agreements",
        "--accept-source-agreements"
    )
}

function Get-RigWindowsArch {
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) {
        $arch = $env:PROCESSOR_ARCHITECTURE
    }

    switch ($arch) {
        "ARM64" { return "arm64" }
        "AMD64" { return "x86_64" }
        "x86_64" { return "x86_64" }
        default { throw "unsupported architecture for rig: $arch" }
    }
}

function Get-GitHubApiHeaders {
    $token = $env:GITHUB_TOKEN
    if (-not $token) {
        $token = $env:GITHUB_PAT
    }
    if (-not $token) {
        return $null
    }

    return @{
        "Accept" = "application/vnd.github+json"
        "Authorization" = "Bearer $token"
        "X-GitHub-Api-Version" = "2022-11-28"
    }
}

function Get-LatestRigReleaseTag {
    $headers = Get-GitHubApiHeaders
    if ($DryRun) {
        if ($headers) {
            Write-Host "+ Invoke-RestMethod -Uri $RigLatestReleaseApi -Headers <github-token>"
        }
        else {
            Write-Host "+ Invoke-RestMethod -Uri $RigLatestReleaseApi"
        }
        return "<latest-rig-tag>"
    }

    if ($headers) {
        $release = Invoke-RestMethod -Uri $RigLatestReleaseApi -Headers $headers
    }
    else {
        $release = Invoke-RestMethod -Uri $RigLatestReleaseApi
    }
    $tag = [string]$release.tag_name
    if (-not $tag) {
        throw "could not resolve latest rig release tag"
    }
    return $tag
}

function Get-RigVersionFromTag {
    param([Parameter(Mandatory = $true)][string]$Tag)

    if ($Tag -eq "<latest-rig-tag>") {
        return "<latest-rig-version>"
    }
    if ($Tag.StartsWith("v", [System.StringComparison]::OrdinalIgnoreCase)) {
        return $Tag.Substring(1)
    }
    return $Tag
}

function Invoke-InstallerStep {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    Write-Step $File $Arguments
    if (-not $DryRun) {
        $process = Start-Process -FilePath $File -ArgumentList $Arguments -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            throw "$File exited with code $($process.ExitCode)"
        }
    }
}

function Install-RigFromGitHubRelease {
    $tag = Get-LatestRigReleaseTag
    $version = Get-RigVersionFromTag $tag
    $arch = Get-RigWindowsArch
    if ($arch -eq "arm64") {
        $asset = "rig-windows-arm64-$version.exe"
    }
    else {
        $asset = "rig-windows-$version.exe"
    }
    $url = "https://github.com/r-lib/rig/releases/download/$tag/$asset"
    if ($DryRun) {
        $installer = Join-Path ([System.IO.Path]::GetTempPath()) "ir-rig-installer.exe"
    }
    else {
        $installer = Join-Path ([System.IO.Path]::GetTempPath()) "ir-rig-installer-$([System.Guid]::NewGuid().ToString('N')).exe"
    }

    Write-Host "+ Invoke-WebRequest -Uri $url -OutFile $installer"
    if (-not $DryRun) {
        Invoke-WebRequest -Uri $url -OutFile $installer
    }

    try {
        Invoke-InstallerStep $installer @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART")
    }
    finally {
        if (-not $DryRun) {
            Remove-Item $installer -Force -ErrorAction SilentlyContinue
        }
    }
}

function Install-Rig {
    if ($env:GITHUB_ACTIONS -eq "true") {
        Install-RigFromGitHubRelease
    }
    else {
        Install-WingetPackage "posit.rig"
    }
}

function Install-Rustup {
    $rustupInit = Join-Path ([System.IO.Path]::GetTempPath()) "rustup-init-$([System.Guid]::NewGuid().ToString('N')).exe"

    Write-Host "+ Invoke-WebRequest -Uri $RustupInitUrl -OutFile $rustupInit"
    if (-not $DryRun) {
        Invoke-WebRequest -Uri $RustupInitUrl -OutFile $rustupInit
    }

    try {
        Invoke-Step $rustupInit @("-y", "--default-toolchain", "stable")
    }
    finally {
        if (-not $DryRun) {
            Remove-Item $rustupInit -Force -ErrorAction SilentlyContinue
        }
    }
}

Add-KnownInstallPaths

if (-not $SkipRust -and -not (Test-Tool "cl")) {
    Require-Tool "winget"
    Invoke-Step "winget" @(
        "install",
        "--id",
        "Microsoft.VisualStudio.2022.BuildTools",
        "--exact",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--override",
        "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    )
}

if (-not $SkipRust -and -not (Test-Tool "cargo")) {
    Install-Rustup
    Add-KnownInstallPaths
}

if (-not $SkipRust -and ($DryRun -or (Test-Tool "rustup"))) {
    Invoke-Step "rustup" @("toolchain", "install", "stable", "--component", "rustfmt", "--component", "clippy")
    Invoke-Step "rustup" @("default", "stable")
}

if (-not $SkipPython -and -not (Test-AnyRunnableTool @("python", "python3"))) {
    Install-WingetPackage "Python.Python.3.13"
    Add-KnownInstallPaths
}

if (-not (Test-Tool "rig")) {
    Install-Rig
    Add-KnownInstallPaths
}

if (-not $SkipQuarto -and -not (Test-Tool "quarto")) {
    Install-WingetPackage "Posit.Quarto"
    Add-KnownInstallPaths
}

if (-not $DryRun -and -not (Test-Tool "rig")) {
    throw "rig is not on PATH after installation; restart PowerShell and rerun this script"
}

if (-not $SkipRRelease) {
    Invoke-Step "rig" @("add", "release")
}
if (-not $SkipTestR) {
    Invoke-Step "rig" @("add", $TestRSpec)
}

Set-TestRMetadata

Invoke-Step "cargo" @("--version")
Invoke-Step "rustc" @("--version")
Invoke-Step (Get-PythonTool) @("--version")
Invoke-Step "rig" @("--version")
Invoke-Step "Rscript" @("--version")
if (-not $SkipTestR) {
    if (-not $TestRName) {
        throw "test R metadata was not loaded"
    }
    Invoke-Step "rig" @("run", "-r", $TestRName, "-e", "stopifnot(as.character(getRversion()) == '$TestRVersion')")
}
Invoke-Step "quarto" @("--version")

if (-not $SkipTestR -and -not $DryRun -and $env:GITHUB_ENV) {
    Add-Content -Path $env:GITHUB_ENV -Value "IR_TEST_R_VERSION=$TestRVersion"
    Add-Content -Path $env:GITHUB_ENV -Value "IR_TEST_R_EXCLUDE_NEWER=$TestRExcludeNewer"
    Add-Content -Path $env:GITHUB_ENV -Value "IR_TEST_RSCRIPT=$TestRscript"
}

Write-Host ""
Write-Host "Developer dependencies are installed."
if ($SkipTestR) {
    return
}
Write-Host "To enable the version-selection tests in this PowerShell session, run:"
Write-Host ""
Write-Host "  `$env:IR_TEST_R_VERSION=$TestRVersion"
Write-Host "  `$env:IR_TEST_R_EXCLUDE_NEWER=$TestRExcludeNewer"
Write-Host "  `$env:IR_TEST_RSCRIPT='$TestRscript'"
Write-Host ""
Write-Host "Then run:"
Write-Host ""
Write-Host "  cargo test"
