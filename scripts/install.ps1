# Install a pre-built `ir` binary on Windows.
#
#   irm https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.ps1 | iex
#
# Downloads the latest Windows release archive, verifies it runs, and installs
# `ir.exe` and `rx.exe` into $env:IR_INSTALL_DIR (default $HOME\bin). The x64
# build runs on both x64 and arm64 Windows (via emulation). The install
# directory is added to the user PATH unless IR_NO_MODIFY_PATH is set.

$ErrorActionPreference = "Stop"

$Owner = "t-kalinowski"
$Repo = "ir"
$App = "ir"
$Target = "x86_64-pc-windows-msvc"

$url = "https://github.com/$Owner/$Repo/releases/latest/download/$App-$Target.zip"
$installDir = if ($env:IR_INSTALL_DIR) { $env:IR_INSTALL_DIR } else { Join-Path $HOME "bin" }

function Get-PathEntries([string]$PathValue) {
    if ([string]::IsNullOrEmpty($PathValue)) {
        return
    }

    $PathValue -split [System.IO.Path]::PathSeparator | Where-Object { $_ -ne "" }
}

function Normalize-PathEntry([string]$PathEntry) {
    try {
        $PathEntry = (Resolve-Path -LiteralPath $PathEntry -ErrorAction Stop).ProviderPath
    } catch {
    }

    try {
        $PathEntry = [System.IO.Path]::GetFullPath($PathEntry)
    } catch {
    }

    return $PathEntry.TrimEnd('\').ToLowerInvariant()
}

function Test-PathContainsDir([string]$PathValue, [string]$Dir) {
    $dirNorm = Normalize-PathEntry $Dir
    foreach ($entry in (Get-PathEntries $PathValue)) {
        if ((Normalize-PathEntry $entry) -eq $dirNorm) {
            return $true
        }
    }

    return $false
}

function Get-ShortPathEntry([string]$PathEntry) {
    try {
        $fileSystem = New-Object -ComObject Scripting.FileSystemObject
        $shortPath = $fileSystem.GetFolder($PathEntry).ShortPath
        if ($shortPath -and $shortPath.Length -lt $PathEntry.Length) {
            return $shortPath
        }
    } catch {
    }

    return $PathEntry
}

function Add-InstallDirToProcessPath([string]$PathEntry, [string]$InstallDir) {
    if (
        (Test-PathContainsDir $env:PATH $PathEntry) -or
        (Test-PathContainsDir $env:PATH $InstallDir)
    ) {
        return
    }

    $env:PATH = ($PathEntry, $env:PATH) -join [System.IO.Path]::PathSeparator
}

function Ensure-InstallDirOnPath([string]$InstallDir, [string]$Commands) {
    if ($env:IR_NO_MODIFY_PATH) {
        if (-not (Test-PathContainsDir $env:PATH $InstallDir)) {
            Write-Host "add $InstallDir to your PATH to run $Commands"
        }
        return
    }

    try {
        $resolvedInstallDir = (Resolve-Path -LiteralPath $InstallDir -ErrorAction Stop).ProviderPath
        $pathEntry = Get-ShortPathEntry $resolvedInstallDir
        $registryPath = 'registry::HKEY_CURRENT_USER\Environment'
        $pathEntries = (Get-Item -LiteralPath $registryPath).GetValue(
            'Path', '', 'DoNotExpandEnvironmentNames') -split ';' -ne ''

        $pathEntryNorm = Normalize-PathEntry $pathEntry
        $installDirNorm = Normalize-PathEntry $resolvedInstallDir
        $pathEntryNorms = $pathEntries | ForEach-Object { Normalize-PathEntry $_ }

        if (($pathEntryNorm -in $pathEntryNorms) -or ($installDirNorm -in $pathEntryNorms)) {
            Add-InstallDirToProcessPath $pathEntry $resolvedInstallDir
            return
        }

        $newPath = (,$pathEntry + $pathEntries) -join ';'
        if ($newPath.Length -gt 32767) {
            Write-Warning (@(
                "Adding $pathEntry would make your user PATH $($newPath.Length) characters,"
                "exceeding the Windows environment variable limit of 32767."
                "Remove stale entries from your user PATH or choose a shorter install directory with IR_INSTALL_DIR."
            ) -join ' ')
            if (-not (Test-PathContainsDir $env:PATH $resolvedInstallDir)) {
                Write-Host "add $InstallDir to your PATH to run $Commands"
            }
            return
        }

        Set-ItemProperty -Type ExpandString -LiteralPath $registryPath Path -Value $newPath

        $dummyName = 'ir-' + [guid]::NewGuid().ToString()
        [Environment]::SetEnvironmentVariable($dummyName, 'ir-dummy', 'User')
        [Environment]::SetEnvironmentVariable($dummyName, $null, 'User')

        Add-InstallDirToProcessPath $pathEntry $resolvedInstallDir
        Write-Host "added $installDir to user PATH"
    } catch {
        Write-Warning "could not add $InstallDir to user PATH: $($_.Exception.Message)"
        if (-not (Test-PathContainsDir $env:PATH $InstallDir)) {
            Write-Host "add $InstallDir to your PATH to run $Commands"
        }
    }
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) "$App-install-$([System.Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp "$App.zip"
    Write-Host "downloading $App-$Target ..."
    Invoke-WebRequest -Uri $url -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath $tmp -Force

    $exe = Join-Path $tmp "$App-$Target\$App.exe"
    $rx = Join-Path $tmp "$App-$Target\rx.exe"
    $hasRx = Test-Path $rx

    # Verify the binary actually runs before installing it.
    & $exe --help | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "downloaded $App-$Target does not run on this system"
    }
    if ($hasRx) {
        & $rx --help | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "downloaded rx from $App-$Target does not run on this system"
        }
    }

    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item $exe (Join-Path $installDir "$App.exe") -Force
    Write-Host "installed $App to $installDir\$App.exe"
    $commands = $App
    if ($hasRx) {
        Copy-Item $rx (Join-Path $installDir "rx.exe") -Force
        Write-Host "installed rx to $installDir\rx.exe"
        $commands = "$App and rx"
    }
    Ensure-InstallDirOnPath $installDir $commands
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
