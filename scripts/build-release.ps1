# Build a release: RideShell App first, then rideshell.exe with the app bundled
# inside it, so a single file installs both.
#   powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo build --release -p rideshell-app
    if ($LASTEXITCODE -ne 0) { throw 'building rideshell-app failed' }
    $env:RIDESHELL_APP_EXE = (Resolve-Path 'target\release\rideshell-app.exe').Path
    cargo build --release -p rideshell-cli
    if ($LASTEXITCODE -ne 0) { throw 'building rideshell failed' }
    $exe = Get-Item 'target\release\rideshell.exe'
    "Built $($exe.FullName) ($([math]::Round($exe.Length / 1MB, 1)) MB, includes RideShell App)"
} finally {
    Remove-Item Env:\RIDESHELL_APP_EXE -ErrorAction SilentlyContinue
    Pop-Location
}
