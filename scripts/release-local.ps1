# scripts/release-local.ps1
# Builds and publishes a release of the fork almatkai/hex entirely from local
# machines (Windows x64 here, macOS arm64 over Tailscale) to GitHub Releases,
# where installed copies of Hex find it and update with one click
# (src/updater/fork.rs, docs/releasing-fork.md). No GitHub Actions.
#
#   scripts\release-local.ps1 -Version 3.4.1 -DryRun   # checks and the plan only
#   scripts\release-local.ps1 -Version 3.4.1           # build, sign, publish
#
# Windows PowerShell 5.1 compatible.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version,

    [ValidateSet('stable', 'beta', 'dev')]
    [string]$Channel = 'stable',

    [switch]$DryRun,

    # Publish Windows only; Macs then see no update in this release.
    [switch]$SkipMac,

    [string]$MacHost = 'almat@100.67.107.38',

    [string]$MacRepo = '~/Desktop/code/hex',

    [string]$Repo = 'almatkai/hex',

    [switch]$AllowAnyBranch
)

$ErrorActionPreference = 'Stop'

# Runs a probe whose diagnostic stderr must not raise NativeCommandError.
function Invoke-Probe([scriptblock]$Command) {
    $prev = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        return (& $Command 2>$null)
    } finally {
        $ErrorActionPreference = $prev
    }
}

# Runs a native command and stops on a nonzero exit.
function Invoke-Native([string]$What, [scriptblock]$Command) {
    & $Command
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

$RepoRoot = if ($PSScriptRoot) { (Resolve-Path (Join-Path $PSScriptRoot '..')).Path } else { (Get-Location).Path }
$Utf8 = New-Object System.Text.UTF8Encoding($false)

if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$') {
    throw "Invalid version '$Version'. Expected semver like 3.4.1 or 3.5.0-beta.1."
}
$tagName = "v$Version"
$pointerTag = "channel-$Channel"
$downloads = "/$Repo/releases/download/$tagName"

# --- Preconditions ---------------------------------------------------------
Write-Host '=========================================================='
Write-Host " Precondition checks (Hex $Version, $Channel channel)"
Write-Host '=========================================================='

$checks = [ordered]@{}
$currentBranch = (git -C $RepoRoot rev-parse --abbrev-ref HEAD).Trim()
$checks["Branch is dev ($currentBranch)"] = ($currentBranch -eq 'dev') -or $AllowAnyBranch
$checks['Working tree is clean'] = [string]::IsNullOrEmpty((git -C $RepoRoot status --porcelain))
Invoke-Probe { git -C $RepoRoot fetch -q origin dev } | Out-Null
$commitSha = (git -C $RepoRoot rev-parse HEAD).Trim()
$originDev = Invoke-Probe { git -C $RepoRoot rev-parse origin/dev }
$checks['HEAD is origin/dev'] = $originDev -and ($commitSha -eq $originDev.Trim())
$checks['GH_TOKEN is set'] = -not [string]::IsNullOrWhiteSpace($env:GH_TOKEN)
$keyPath = Join-Path $RepoRoot 'secrets\update-signing.key'
$checks['Update signing key (secrets\update-signing.key)'] = (Test-Path $keyPath) -and ((Get-Item $keyPath).Length -gt 0)
$secretPath = Join-Path $RepoRoot 'secrets\antigravity-client-secret.txt'
$checks['Antigravity client secret (secrets\)'] = (Test-Path $secretPath) -and ((Get-Item $secretPath).Length -gt 0)
if (-not $SkipMac) {
    Invoke-Probe { & ssh -o ConnectTimeout=5 -o BatchMode=yes -n $MacHost "test -s $MacRepo/secrets/antigravity-client-secret.txt" } | Out-Null
    $checks["Mac reachable with the Antigravity secret ($MacHost)"] = ($LASTEXITCODE -eq 0)
}
$tagExists = (git -C $RepoRoot tag -l $tagName) -or (Invoke-Probe { git -C $RepoRoot ls-remote --tags origin "refs/tags/$tagName" })
$checks["Tag $tagName is new"] = -not $tagExists

$failed = $false
foreach ($name in $checks.Keys) {
    if ($checks[$name]) {
        Write-Host " [PASS] $name" -ForegroundColor Green
    } else {
        Write-Host " [FAIL] $name" -ForegroundColor Red
        $failed = $true
    }
}

# --- Release notes ---------------------------------------------------------
$prevTag = Invoke-Probe { git -C $RepoRoot describe --tags --abbrev=0 --match 'v*' }
$range = if ($prevTag) { "$($prevTag.Trim())..HEAD" } else { '-n50' }
$log = @(git -C $RepoRoot log $range --pretty=format:'* %s (%h)')
$notes = if ($prevTag) { "## Changes since $($prevTag.Trim())`n`n" } else { "## Hex $Version`n`n" }
$notes += ($log -join "`n")
$notes += "`n`n## Install`n`nWindows (PowerShell):`n`n    irm https://github.com/$Repo/releases/latest/download/install-hex.ps1 | iex`n`nmacOS: open the .dmg and drag Hex to Applications.`n`nInstalled copies update themselves: Hex shows Restart to update."

if ($DryRun) {
    Write-Host ''
    Write-Host '=========================================================='
    Write-Host " [DRY RUN] Plan for Hex $Version ($Channel)"
    Write-Host '=========================================================='
    Write-Host " Commit  : $commitSha"
    Write-Host " Release : $Repo $tagName, then pointer $pointerTag/$Channel.json"
    Write-Host ' 1. Windows: cargo build --release (hex, hex-console), HEX_BUILD_CHANNEL, updates on'
    if ($SkipMac) { Write-Host ' 2. macOS: skipped' } else { Write-Host " 2. macOS on ${MacHost}: build-app.sh (HEX_RELEASE_UNSIGNED=1), zip + dmg" }
    Write-Host ' 3. Sign every file with secrets\update-signing.key (<file>.sig)'
    Write-Host " 4. Write $Channel.json (schema 1), publish $tagName, move $pointerTag"
    Write-Host ''
    Write-Host $notes
    Write-Host ''
    if ($failed) { Write-Host '[DRY RUN] Some checks failed; a real run would stop here.' -ForegroundColor Yellow }
    else { Write-Host '[DRY RUN] All checks passed.' -ForegroundColor Green }
    return
}
if ($failed) { throw 'Preconditions failed (see above).' }

$dist = Join-Path (Join-Path $RepoRoot 'dist') $Version
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Force $dist | Out-Null

# --- 1. Windows ------------------------------------------------------------
Write-Host ''
Write-Host '==> [1/4] Building Windows x64...' -ForegroundColor Cyan
foreach ($name in 'HEX_LOCAL_BUILD', 'HEX_TEST_BUILD') {
    if (Test-Path "env:$name") { Remove-Item "env:$name" }
}
$env:TRANSCRIBE_CMAKE_ARGS = '-DGGML_NATIVE=OFF'
$env:HEX_VERSION = $Version
$env:HEX_BUILD_CHANNEL = $Channel
$triple = 'x86_64-pc-windows-msvc'
Push-Location $RepoRoot
try {
    Invoke-Native 'cargo build (hex)' { cargo build --locked --release --target $triple }
    Invoke-Native 'cargo build (hex-console)' { cargo build --locked --release --target $triple -p hex-console }
} finally {
    Pop-Location
}
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot 'target' }
$built = Join-Path $targetDir "$triple\release"
$winExe = Join-Path $dist "hex-$Version-windows-x64.exe"
$winCli = Join-Path $dist "hex-$Version-windows-x64-cli.exe"
Copy-Item (Join-Path $built 'hex.exe') $winExe
Copy-Item (Join-Path $built 'hex-console.exe') $winCli
$reported = (& (Join-Path $built 'hex-console.exe') --version) -split '\s+' | Select-Object -Last 1
if ($reported -ne $Version) { throw "The Windows build reports $reported, expected $Version." }
Copy-Item (Join-Path $RepoRoot 'scripts\install-hex.ps1') (Join-Path $dist 'install-hex.ps1')

# --- 2. macOS --------------------------------------------------------------
$macZip = Join-Path $dist "hex-$Version-macos-arm64.zip"
$macDmg = Join-Path $dist "hex-$Version-macos-arm64.dmg"
if (-not $SkipMac) {
    Write-Host ''
    Write-Host "==> [2/4] Building macOS arm64 on $MacHost..." -ForegroundColor Cyan
    $remoteScript = "/tmp/hex-release-$Version.sh"
    $bash = @(
        '#!/usr/bin/env bash',
        'set -euo pipefail',
        'export PATH="/opt/homebrew/bin:/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"',
        "cd $MacRepo",
        'test -s secrets/antigravity-client-secret.txt || { echo "secrets/antigravity-client-secret.txt is missing" >&2; exit 1; }',
        'if [ -n "$(git status --porcelain --untracked-files=no)" ]; then echo "the Mac checkout has uncommitted changes" >&2; exit 1; fi',
        'previous=$(git symbolic-ref --short -q HEAD || git rev-parse HEAD)',
        'trap ''git checkout -q "$previous" || true'' EXIT',
        'git fetch -q origin',
        "git checkout -q $commitSha",
        'export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer',
        "export HEX_VERSION=$Version HEX_BUILD_CHANNEL=$Channel HEX_RELEASE_UNSIGNED=1",
        'unset HEX_LOCAL_BUILD HEX_TEST_BUILD',
        './scripts/build-app.sh',
        'mkdir -p dist',
        "zip=dist/hex-$Version-macos-arm64.zip",
        "dmg=dist/hex-$Version-macos-arm64.dmg",
        'rm -f "$zip" "$dmg"',
        '/usr/bin/ditto -c -k --keepParent target/app/Hex.app "$zip"',
        'staging=$(mktemp -d "${TMPDIR:-/tmp}/hex-dmg.XXXXXX")',
        '/usr/bin/ditto target/app/Hex.app "$staging/Hex.app"',
        'ln -s /Applications "$staging/Applications"',
        'for attempt in 1 2 3 4 5; do hdiutil create -quiet -fs APFS -format ULFO -volname Hex -srcfolder "$staging" "$dmg" && break; rm -f "$dmg"; sleep 2; done',
        'rm -rf "$staging"',
        'test -s "$dmg"',
        'ls -l "$zip" "$dmg"'
    )
    $local = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllText($local, (($bash -join "`n") + "`n"), $Utf8)
    try {
        Invoke-Native 'scp of the Mac build script' { scp -q $local "${MacHost}:$remoteScript" }
        Invoke-Native 'the Mac build' { ssh -n $MacHost "bash $remoteScript" }
    } finally {
        Remove-Item $local -Force -ErrorAction SilentlyContinue
        Invoke-Probe { ssh -n $MacHost "rm -f $remoteScript" } | Out-Null
    }
    Invoke-Native 'download of the Mac zip' { scp -q "${MacHost}:$MacRepo/dist/hex-$Version-macos-arm64.zip" $macZip }
    Invoke-Native 'download of the Mac dmg' { scp -q "${MacHost}:$MacRepo/dist/hex-$Version-macos-arm64.dmg" $macDmg }
} else {
    Write-Host ''
    Write-Host '==> [2/4] macOS skipped' -ForegroundColor Yellow
}

# --- 3. Sign ---------------------------------------------------------------
Write-Host ''
Write-Host '==> [3/4] Signing with the update key...' -ForegroundColor Cyan
# target name -> file; each gets <target>-sig in the manifest.
$targets = [ordered]@{
    'windows-x64'     = $winExe
    'windows-x64-cli' = $winCli
}
if (-not $SkipMac) {
    $targets['macos-arm64'] = $macZip
    $targets['macos-arm64-dmg'] = $macDmg
}
$signer = Join-Path $built 'hex-console.exe'
$files = @($targets.Values)
Invoke-Native 'release-sign' { & $signer release-sign --key $keyPath @files }
foreach ($file in $files) {
    if (-not (Test-Path "$file.sig") -or (Get-Item "$file.sig").Length -ne 64) { throw "No signature for $file." }
}

# --- 4. Manifest and publish -----------------------------------------------
Write-Host ''
Write-Host "==> [4/4] Publishing $tagName and moving $pointerTag..." -ForegroundColor Cyan
function Entry([string]$path) {
    [ordered]@{
        url    = "$downloads/$(Split-Path -Leaf $path)"
        size   = (Get-Item $path).Length
        sha256 = (Get-FileHash -Algorithm SHA256 $path).Hash.ToLowerInvariant()
    }
}
$manifestTargets = [ordered]@{}
foreach ($name in $targets.Keys) {
    $manifestTargets[$name] = Entry $targets[$name]
    $manifestTargets["$name-sig"] = Entry "$($targets[$name]).sig"
}
$now = [DateTimeOffset]::UtcNow
$manifest = [ordered]@{
    schema       = 1
    app          = 'hex'
    artifact     = 'desktop'
    channel      = $Channel
    sequence     = $now.ToUnixTimeSeconds()
    published_at = $now.ToString('yyyy-MM-ddTHH:mm:ssZ')
    release      = [ordered]@{
        version         = $Version
        notes_url       = "https://github.com/$Repo/releases/tag/$tagName"
        rollout_percent = 100
        required        = $false
        distributions   = @(@{ kind = 'direct' })
        targets         = $manifestTargets
    }
}
$manifestPath = Join-Path $dist "$Channel.json"
[System.IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 8), $Utf8)

$sums = Get-ChildItem $dist -File | Where-Object { $_.Name -ne 'SHA256SUMS' } | ForEach-Object {
    "$((Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLowerInvariant())  $($_.Name)"
}
[System.IO.File]::WriteAllLines((Join-Path $dist 'SHA256SUMS'), [string[]]$sums, $Utf8)

$notesFile = [System.IO.Path]::GetTempFileName()
[System.IO.File]::WriteAllText($notesFile, $notes, $Utf8)
try {
    $assets = @(Get-ChildItem $dist -File | ForEach-Object { $_.FullName })
    $kind = if ($Channel -eq 'stable') { '--latest' } else { '--prerelease' }
    # Files first: the pointer only moves once everything it names exists.
    Invoke-Native "gh release create $tagName" {
        gh release create $tagName --repo $Repo --target $commitSha --title "Hex $Version" --notes-file $notesFile $kind @assets
    }
    Invoke-Probe { gh release view $pointerTag --repo $Repo --json tagName } | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Invoke-Native "gh release create $pointerTag" {
            gh release create $pointerTag --repo $Repo --target $commitSha --prerelease --title "Hex $Channel update feed" --notes "Update feed for Hex's $Channel channel. Installed copies read $Channel.json here; download Hex from the versioned releases."
        }
    }
    Invoke-Native "gh release upload $pointerTag" { gh release upload $pointerTag $manifestPath --repo $Repo --clobber }
} finally {
    Remove-Item $notesFile -Force -ErrorAction SilentlyContinue
}
Write-Host ''
Write-Host "Published Hex $Version on the $Channel channel: https://github.com/$Repo/releases/tag/$tagName" -ForegroundColor Green
Write-Host 'Installed copies offer it within an hour, or at once from Check for updates.'
