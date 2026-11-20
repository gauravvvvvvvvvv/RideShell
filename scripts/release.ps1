# One-step release: bump the version, test, build Windows + Linux, sign, then
# (after you confirm) commit, push and publish the GitHub release.
#
#   scripts\release.cmd                 next patch version (0.1.5 -> 0.1.6)
#   scripts\release.cmd 0.2.0           a version of your choice
#   scripts\release.cmd -DryRun         everything except publishing; changes are undone
#
# Needs: git, gh (signed in), Docker Desktop (for the Linux build) and the
# signing key (~/.rideshell-release/signing.key).
#
# Release notes: whatever is under "## Unreleased" in CHANGELOG.md. With no
# such section, the commit messages since the last release are used (and
# added to the changelog).
param([string]$Version = '', [switch]$DryRun)
# Native tools (cargo, docker, gh) write progress to stderr; failures are
# detected from exit codes instead.
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }
function Fail($text) { throw $text }
function Run($what, [scriptblock]$cmd) { & $cmd; if ($LASTEXITCODE -ne 0) { Fail "$what failed" } }

$versioned = 'Cargo.toml', 'Cargo.lock', 'CHANGELOG.md'
$undo = @()
$code = 0
try {
    # ----- checks before touching anything -----
    Step 'Checking'
    $gh = (Get-Command gh -ErrorAction SilentlyContinue).Source
    if (-not $gh) { $gh = 'C:\Program Files\GitHub CLI\gh.exe' }
    if (-not (Test-Path $gh)) { Fail 'GitHub CLI (gh) is not installed' }
    if ($DryRun) {
        if (git status --porcelain -- $versioned) { Fail 'Cargo.toml, Cargo.lock or CHANGELOG.md has uncommitted changes' }
    } else {
        if (git status --porcelain) { Fail 'there are uncommitted changes; commit them first' }
        if ((git branch --show-current) -ne 'main') { Fail 'switch to the main branch first' }
    }
    $key = if ($env:RIDESHELL_SIGNING_KEY) { $env:RIDESHELL_SIGNING_KEY } else { Join-Path $HOME '.rideshell-release\signing.key' }
    if (-not (Test-Path $key)) { Fail "signing key not found at $key" }

    $toml = [IO.File]::ReadAllText("$root\Cargo.toml")
    $m = [regex]::Match($toml, '(?m)^version = "(\d+)\.(\d+)\.(\d+)"')
    if (-not $m.Success) { Fail 'no version in Cargo.toml' }
    $old = '{0}.{1}.{2}' -f $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[3].Value
    if (-not $Version) { $Version = '{0}.{1}.{2}' -f $m.Groups[1].Value, $m.Groups[2].Value, ([int]$m.Groups[3].Value + 1) }
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { Fail "the version must look like 1.2.3 (got $Version)" }
    if ([version]$Version -le [version]$old) { Fail "$Version is not newer than $old (installed copies only accept newer versions)" }
    & $gh release view "v$Version" *> $null
    if ($LASTEXITCODE -eq 0) { Fail "release v$Version already exists" }

    $changelog = [IO.File]::ReadAllText("$root\CHANGELOG.md")
    $hasNotes = $changelog -match '(?m)^## Unreleased\s*$'

    docker version --format '{{.Server.Version}}' *> $null
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'Starting Docker Desktop...'
        Start-Process 'C:\Program Files\Docker\Docker\Docker Desktop.exe'
        for ($i = 0; $i -lt 60; $i++) { Start-Sleep 3; docker version --format '{{.Server.Version}}' *> $null; if ($LASTEXITCODE -eq 0) { break } }
        if ($LASTEXITCODE -ne 0) { Fail 'Docker is not running (needed for the Linux build)' }
    }
    Write-Host "Releasing $old -> $Version$(if ($DryRun) { ' (dry run: nothing is published)' })"

    # ----- version -----
    Step "Version $Version"
    $undo = $versioned
    [IO.File]::WriteAllText("$root\Cargo.toml", ([regex]'(?m)^version = "[\d.]+"').Replace($toml, "version = `"$Version`"", 1))
    $heading = "## $Version ($(Get-Date -Format 'yyyy-MM-dd'))"
    if ($hasNotes) {
        $changelog = ([regex]'(?m)^## Unreleased[ \t]*\r?$').Replace($changelog, $heading, 1)
    } else {
        # No notes written: list the commits since the last release.
        git fetch --quiet --tags 2>$null
        $commits = git log --format='* %s' "v$old..HEAD" 2>$null | Where-Object { $_ -notmatch '^\* RideShell \d' }
        if (-not $commits) { $commits = @('* Maintenance release.') }
        $nl = if ($changelog.Contains("`r`n")) { "`r`n" } else { "`n" }
        $section = "$heading$nl$nl$($commits -join $nl)$nl$nl"
        $first = [regex]::Match($changelog, '(?m)^## ')
        $changelog = if ($first.Success) { $changelog.Insert($first.Index, $section) } else { $changelog + "`n" + $section }
    }
    [IO.File]::WriteAllText("$root\CHANGELOG.md", $changelog)
    $notes = [regex]::Match($changelog, "(?ms)^$([regex]::Escape($heading))\s*(.*?)(?=^## |\z)").Groups[1].Value.Trim()
    if (-not $notes) { $notes = "RideShell $Version" }

    # ----- test -----
    Step 'Tests'
    foreach ($v in 'RIDESHELL_SESSION', 'RIDESHELL_SHELL', 'RIDESHELL_PROFILE', 'RIDESHELL_ACTIVE_PROFILE', 'RIDESHELL_STATE') { Remove-Item "env:$v" -ErrorAction SilentlyContinue }
    Run 'the tests' { cargo test --workspace --quiet }

    # ----- build -----
    $dist = Join-Path $root "target\dist\$Version"
    Remove-Item -Recurse -Force $dist -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $dist | Out-Null

    Step 'Windows build'
    Run 'the Windows build' { powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-release.ps1 }
    Copy-Item target\release\rideshell.exe $dist -ErrorAction Stop

    Step 'Linux build (Docker)'
    $reg = Join-Path $HOME '.cargo\registry'
    # The container's network is unreliable: seed its cargo cache from this PC, then build offline.
    Run 'seeding the cargo cache' { docker run --rm -v "${reg}:/host-reg:ro" -v rideshell-cargo-reg:/usr/local/cargo/registry alpine sh -c 'mkdir -p /usr/local/cargo/registry/cache /usr/local/cargo/registry/index; cp -rn /host-reg/cache/. /usr/local/cargo/registry/cache/; cp -rn /host-reg/index/. /usr/local/cargo/registry/index/' }
    $pack = 'set -e; cargo build --locked --release -p rideshell-cli; d=$(mktemp -d); cp /target/release/rideshell "$d/"; tr -d "\r" < LICENSE > "$d/LICENSE"; tr -d "\r" < README.md > "$d/README.md"; chmod 755 "$d"/*; tar -C "$d" --owner=root --group=root -czf /out/rideshell-linux-x86_64.tar.gz rideshell LICENSE README.md'
    Run 'the Linux build' { docker run --rm -v "${root}:/src:ro" -v "${dist}:/out" -v rideshell-linux-target:/target -v rideshell-cargo-reg:/usr/local/cargo/registry -e CARGO_TARGET_DIR=/target -e CARGO_NET_OFFLINE=true -w /src rust:1-bullseye bash -c $pack }

    # ----- sign -----
    Step 'Signing'
    Run 'signing' { cargo run --quiet -p rideshell-release -- sign $dist }
    $built = (& "$dist\rideshell.exe" --version) -replace '^\S+\s+', ''
    if ($built -ne $Version) { Fail "the built exe says $built, expected $Version" }

    Step 'Ready'
    Get-ChildItem $dist | ForEach-Object { '  {0,-38} {1,8:N2} MB' -f $_.Name, ($_.Length / 1MB) }
    Write-Host "`nRelease notes:" -ForegroundColor Cyan
    Write-Host $notes
    if ($DryRun) { Write-Host "`nDry run finished: built and signed in $dist. Nothing was published." -ForegroundColor Green; return }

    $answer = Read-Host "`nPublish RideShell $Version to GitHub now? [y/N]"
    if ($answer -notmatch '^(y|yes)$') { Write-Host "Not published. The files are in $dist"; return }

    # ----- publish -----
    Step 'Publishing'
    Run 'git commit' { git commit --quiet -m "RideShell $Version" -- $versioned }
    $undo = @() # committed: keep the version bump
    Run 'git push' { git push --quiet origin main }
    $notesFile = Join-Path $root "target\dist\notes-$Version.md"
    [IO.File]::WriteAllText($notesFile, $notes)
    Run 'creating the GitHub release' { & $gh release create "v$Version" (Get-ChildItem $dist).FullName --target main --title "RideShell $Version" --notes-file $notesFile }
    Write-Host "`nPublished RideShell $Version. Installed copies will offer the update." -ForegroundColor Green
}
catch {
    Write-Host "`nStopped: $_" -ForegroundColor Red
    $code = 1
}
finally {
    # A dry run, a failure or "no": put the version back as it was.
    if ($undo.Count) { git checkout --quiet -- $undo }
}
exit $code
