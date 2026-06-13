# Install workstation dependencies for building and testing ir on Windows.
# This installs system tools only; the first test run still owns the ir/R
# package cache warm-up.

[CmdletBinding()]
param(
    [switch]$DryRun,
    [string[]]$Skip = @()
)

$ErrorActionPreference = "Stop"

$TestRVersion = "4.4.3"
$RustupInitUrl = "https://win.rustup.rs"
$SkipRust = $false
$SkipPython = $false
$SkipQuarto = $false
$SkipRRelease = $false

foreach ($component in $Skip) {
    switch ($component) {
        "rust" { $SkipRust = $true }
        "python" { $SkipPython = $true }
        "quarto" { $SkipQuarto = $true }
        "r-release" { $SkipRRelease = $true }
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

function Install-Rig {
    if ($env:GITHUB_ACTIONS -eq "true") {
        Require-Tool "choco"
        Invoke-Step "choco" @("install", "rig", "-y", "--no-progress")
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
Invoke-Step "rig" @("add", $TestRVersion)
if (-not $SkipRRelease) {
    Invoke-Step "rig" @("default", "release")
}

Invoke-Step "cargo" @("--version")
Invoke-Step "rustc" @("--version")
Invoke-Step (Get-PythonTool) @("--version")
Invoke-Step "rig" @("--version")
Invoke-Step "Rscript" @("--version")
Invoke-Step "rig" @("run", "-r", $TestRVersion, "-e", "stopifnot(as.character(getRversion()) == '$TestRVersion')")
Invoke-Step "quarto" @("--version")

Write-Host ""
Write-Host "Developer dependencies are installed."
Write-Host "To enable the version-selection tests in this PowerShell session, run:"
Write-Host ""
Write-Host "  `$env:IR_TEST_R_VERSION=4.4.3"
Write-Host ""
Write-Host "Then run:"
Write-Host ""
Write-Host "  cargo test"
