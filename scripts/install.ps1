# Install a pre-built `ir` binary on Windows.
#
#   irm https://raw.githubusercontent.com/t-kalinowski/ir/main/scripts/install.ps1 | iex
#
# Downloads the latest Windows release archive, verifies it runs, and installs
# `ir.exe` and `rx.exe` into $env:IR_INSTALL_DIR (default $HOME\bin). The x64
# build runs on both x64 and arm64 Windows (via emulation).

$ErrorActionPreference = "Stop"

$Owner = "t-kalinowski"
$Repo = "ir"
$App = "ir"
$Target = "x86_64-pc-windows-msvc"

$url = "https://github.com/$Owner/$Repo/releases/latest/download/$App-$Target.zip"
$installDir = if ($env:IR_INSTALL_DIR) { $env:IR_INSTALL_DIR } else { Join-Path $HOME "bin" }

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
    $pathDirs = $env:PATH -split [System.IO.Path]::PathSeparator
    if ($pathDirs -notcontains $installDir) {
        Write-Host "add $installDir to your PATH to run $commands"
    }
}
finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
