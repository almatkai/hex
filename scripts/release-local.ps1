# scripts/release-local.ps1
# Builds and publishes a multi-platform release of the fork almatkai/hex
# entirely from local machines (Windows x64 local + macOS arm64 over Tailscale).
# Fully compatible with Windows PowerShell 5.1.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version,

    [switch]$DryRun,

    [string]$MacHost = 'almat@100.67.107.38',

    [string]$MacRepo = '~/Desktop/code/hex',

    [string]$Repo = 'almatkai/hex',

    [switch]$AllowAnyBranch
)

$ErrorActionPreference = 'Stop'

# Helper for probing commands whose diagnostic stderr must not trigger PowerShell's NativeCommandError
function Invoke-Probe([scriptblock]$Command) {
    $prev = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        return (& $Command 2>$null)
    } finally {
        $ErrorActionPreference = $prev
    }
}

# Determine repository root
$RepoRoot = if ($PSScriptRoot) {
    (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
} else {
    (Get-Location).Path
}

# 1. Parameter validation
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$') {
    throw "Invalid version format '$Version'. Expected semver format like 3.4.1 or 0.0.0-test."
}

$tagName = "v$Version"

# 2. Precondition checking
Write-Host "=========================================================="
Write-Host " Precondition Checks"
Write-Host "=========================================================="

# Check current branch
$currentBranch = (git -C $RepoRoot rev-parse --abbrev-ref HEAD).Trim()
$branchOk = ($currentBranch -eq 'dev') -or $AllowAnyBranch

# Check working tree cleanliness
$dirtyStatus = (git -C $RepoRoot status --porcelain)
$treeClean = [string]::IsNullOrEmpty($dirtyStatus)

# Check up to date with origin/dev
Invoke-Probe { git -C $RepoRoot fetch origin dev } | Out-Null
$commitSha = (git -C $RepoRoot rev-parse HEAD).Trim()
$originDevSha = ""
$rawOriginDev = (Invoke-Probe { git -C $RepoRoot rev-parse origin/dev })
if ($rawOriginDev) {
    $originDevSha = $rawOriginDev.Trim()
}
$upToDate = ($commitSha -eq $originDevSha)

# Check $env:GH_TOKEN set (never print token value)
$ghTokenSet = -not [string]::IsNullOrWhiteSpace($env:GH_TOKEN)

# Check local secret exists and is non-empty (never print secret content)
$localSecretPath = Join-Path $RepoRoot 'secrets\antigravity-client-secret.txt'
$localSecretOk = (Test-Path $localSecretPath) -and ((Get-Item $localSecretPath).Length -gt 0)

# Check remote secret exists over Tailscale (never print secret content)
$remoteSecretOk = $false
Invoke-Probe { & ssh -o ConnectTimeout=5 -n $MacHost "test -s $MacRepo/secrets/antigravity-client-secret.txt" } | Out-Null
if ($LASTEXITCODE -eq 0) {
    $remoteSecretOk = $true
}

# Check if tag already exists locally or on remote
$tagExists = $false
$localTag = (git -C $RepoRoot tag -l $tagName)
if ($localTag) {
    $tagExists = $true
} else {
    $remoteTag = (Invoke-Probe { git -C $RepoRoot ls-remote --tags origin "refs/tags/$tagName" })
    if ($remoteTag) {
        $tagExists = $true
    }
}
$tagOk = -not $tagExists

# Output precondition results
if ($branchOk) {
    Write-Host " [PASS] Branch: '$currentBranch'" -ForegroundColor Green
} else {
    Write-Host " [FAIL] Branch: current is '$currentBranch' (expected 'dev')" -ForegroundColor Red
}

if ($treeClean) {
    Write-Host " [PASS] Working tree: clean" -ForegroundColor Green
} else {
    Write-Host " [FAIL] Working tree: uncommitted changes detected" -ForegroundColor Red
}

if ($upToDate) {
    Write-Host " [PASS] Up to date: commit matches origin/dev ($commitSha)" -ForegroundColor Green
} else {
    Write-Host " [FAIL] Up to date: local commit ($commitSha) does not match origin/dev ($originDevSha)" -ForegroundColor Red
}

if ($ghTokenSet) {
    Write-Host " [PASS] GH_TOKEN: environment variable is set" -ForegroundColor Green
} else {
    Write-Host " [FAIL] GH_TOKEN: environment variable is not set" -ForegroundColor Red
}

if ($localSecretOk) {
    Write-Host " [PASS] Local secret: present and non-empty (secrets\antigravity-client-secret.txt)" -ForegroundColor Green
} else {
    Write-Host " [FAIL] Local secret: missing or empty (secrets\antigravity-client-secret.txt)" -ForegroundColor Red
}

if ($remoteSecretOk) {
    Write-Host " [PASS] Remote secret: present on ${MacHost}:$MacRepo/secrets/antigravity-client-secret.txt" -ForegroundColor Green
} else {
    Write-Host " [FAIL] Remote secret: missing, empty, or host unreachable on ${MacHost}:$MacRepo" -ForegroundColor Red
}

if ($tagOk) {
    Write-Host " [PASS] Release tag: '$tagName' does not already exist" -ForegroundColor Green
} else {
    Write-Host " [FAIL] Release tag: '$tagName' already exists" -ForegroundColor Red
}

# In live mode, fail immediately if any precondition is not satisfied
if (-not $DryRun) {
    if (-not $branchOk) {
        throw "Precondition failed: Current branch is '$currentBranch', expected 'dev'. (Use -AllowAnyBranch to override)."
    }
    if (-not $treeClean) {
        throw "Precondition failed: Working tree is not clean. Commit or stash changes before releasing."
    }
    if (-not $upToDate) {
        throw "Precondition failed: Local HEAD does not match origin/dev."
    }
    if (-not $ghTokenSet) {
        throw "Precondition failed: GH_TOKEN environment variable must be set (do not print it)."
    }
    if (-not $localSecretOk) {
        throw "Precondition failed: Local secret file 'secrets\antigravity-client-secret.txt' is missing or empty."
    }
    if (-not $remoteSecretOk) {
        throw "Precondition failed: Remote secret file on $MacHost at '$MacRepo/secrets/antigravity-client-secret.txt' is missing or empty."
    }
    if (-not $tagOk) {
        throw "Precondition failed: Release tag '$tagName' already exists."
    }
}

# 3. Generate Release Notes from git log since previous tag
$prevTagRaw = (Invoke-Probe { git -C $RepoRoot describe --tags --abbrev=0 })
$prevTag = if ($prevTagRaw) { $prevTagRaw.Trim() } else { "" }
$notesContent = ""
if ($prevTag) {
    $rawLog = (git -C $RepoRoot log "$prevTag..HEAD" --pretty=format:"* %s (%h)")
    if (-not $rawLog) {
        $notesContent = "Hex release $Version (commit $commitSha)"
    } else {
        $formattedLog = $rawLog -join "`n"
        $notesContent = "## What's Changed since $prevTag`n`n$formattedLog"
    }
} else {
    $rawLog = (git -C $RepoRoot log -n 50 --pretty=format:"* %s (%h)")
    $formattedLog = $rawLog -join "`n"
    $notesContent = "## Hex $Version`n`n$formattedLog"
}

# 4. Dry Run Execution
if ($DryRun) {
    Write-Host ""
    Write-Host "=========================================================="
    Write-Host " [DRY RUN] Release Plan for Hex $Version"
    Write-Host "=========================================================="
    Write-Host "Commit SHA   : $commitSha"
    Write-Host "Repository   : $Repo"
    Write-Host "Release Tag  : $tagName"
    Write-Host "Previous Tag : $(if ($prevTag) { $prevTag } else { 'none' })"
    Write-Host ""
    Write-Host "1. Windows Build (Local):"
    Write-Host "   - Set `$env:HEX_LOCAL_BUILD = '1' (fork builds never self-update to upstream releases)"
    Write-Host "   - Set `$env:TRANSCRIBE_CMAKE_ARGS = '-DGGML_NATIVE=OFF'"
    Write-Host "   - Set `$env:HEX_VERSION = '$Version'"
    Write-Host "   - cargo build --locked --release --target x86_64-pc-windows-msvc"
    Write-Host "   - cargo build --locked --release --target x86_64-pc-windows-msvc -p hex-console"
    Write-Host "   - Package target\x86_64-pc-windows-msvc\release\hex.exe and hex-console.exe (as hex.com)"
    Write-Host "     into dist\$Version\hex-$Version-windows-x64.zip with package SHA256SUMS"
    Write-Host ""
    Write-Host "2. macOS Build (Remote over Tailscale):"
    Write-Host "   - Host: $MacHost"
    Write-Host "   - Path: $MacRepo"
    Write-Host "   - Generate bash build script with LF and no BOM"
    Write-Host "   - scp script to ${MacHost}:/tmp/hex-release-$Version.sh"
    Write-Host "   - Execute via ssh:"
    Write-Host "       refuse a dirty checkout; git fetch origin && git checkout $commitSha (switched back afterwards)"
    Write-Host "       export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer"
    Write-Host "       export HEX_VERSION=$Version"
    Write-Host "       export HEX_RELEASE_UNSIGNED=1 HEX_LOCAL_BUILD=1"
    Write-Host "       ./scripts/build-app.sh"
    Write-Host "       /usr/bin/ditto -c -k --keepParent target/app/Hex.app dist/hex-$Version-macos-arm64.zip"
    Write-Host "       Package DMG if scripts/package-dmg.sh or hdiutil available"
    Write-Host "   - scp dist/hex-$Version-macos-arm64.zip (and DMG if created) back to dist\$Version\"
    Write-Host ""
    Write-Host "3. Publish to GitHub Release:"
    Write-Host "   - Compute dist\$Version\SHA256SUMS for this release's artifacts only"
    Write-Host "   - Create release with gh CLI:"
    Write-Host "       gh release create $tagName --repo $Repo --target $commitSha --title `"Hex $Version`" --notes-file <notes> <artifacts...>"
    Write-Host ""
    Write-Host "Generated Release Notes Preview:"
    Write-Host "----------------------------------------------------------"
    Write-Host $notesContent
    Write-Host "----------------------------------------------------------"
    Write-Host ""

    $hasFailures = (-not $branchOk) -or (-not $treeClean) -or (-not $upToDate) -or (-not $ghTokenSet) -or (-not $localSecretOk) -or (-not $remoteSecretOk) -or (-not $tagOk)
    if ($hasFailures) {
        Write-Host "[DRY RUN] One or more preconditions failed (see above). In a live run, the script would abort here." -ForegroundColor Yellow
    } else {
        Write-Host "[DRY RUN] All preconditions passed. Ready for live release!" -ForegroundColor Green
    }
    return
}

# 5. Live Release Execution

# Step 1: Windows build
Write-Host ""
Write-Host "==> [1/3] Building Windows x64 release..." -ForegroundColor Cyan

# A fork build must never replace itself with an upstream (Anomaly) release:
# HEX_LOCAL_BUILD=1 turns the updater off ("Local build · updates off").
$env:HEX_LOCAL_BUILD = '1'
$env:TRANSCRIBE_CMAKE_ARGS = '-DGGML_NATIVE=OFF'
$env:HEX_VERSION = $Version

$targetTriple = 'x86_64-pc-windows-msvc'

Write-Host "Running: cargo build --locked --release --target $targetTriple"
& cargo build --locked --release --target $targetTriple
if ($LASTEXITCODE -ne 0) {
    throw "cargo build for hex failed with exit code $LASTEXITCODE"
}

Write-Host "Running: cargo build --locked --release --target $targetTriple -p hex-console"
& cargo build --locked --release --target $targetTriple -p hex-console
if ($LASTEXITCODE -ne 0) {
    throw "cargo build for hex-console failed with exit code $LASTEXITCODE"
}

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot 'target' }
$hexExe = Join-Path $targetDir "$targetTriple\release\hex.exe"
$hexConsoleExe = Join-Path $targetDir "$targetTriple\release\hex-console.exe"

if (-not (Test-Path $hexExe)) {
    throw "Missing built hex.exe at $hexExe"
}
if (-not (Test-Path $hexConsoleExe)) {
    throw "Missing built hex-console.exe at $hexConsoleExe"
}

# One folder per release, so a publish never uploads an older release's files.
$distDir = Join-Path (Join-Path $RepoRoot 'dist') $Version
if (Test-Path $distDir) {
    Remove-Item $distDir -Recurse -Force
}
New-Item -ItemType Directory -Path $distDir -Force | Out-Null

$winStaging = Join-Path ([System.IO.Path]::GetTempPath()) ("hex-win-stage-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $winStaging -Force | Out-Null

try {
    Copy-Item $hexExe (Join-Path $winStaging 'hex.exe') -Force
    Copy-Item $hexConsoleExe (Join-Path $winStaging 'hex.com') -Force

    $winFiles = @('hex.exe', 'hex.com')
    $winSums = @()
    foreach ($binName in $winFiles) {
        $binPath = Join-Path $winStaging $binName
        $hashVal = (Get-FileHash -Path $binPath -Algorithm SHA256).Hash.ToLowerInvariant()
        $winSums += "$hashVal  $binName"
    }
    $winSumsPath = Join-Path $winStaging 'SHA256SUMS'
    [System.IO.File]::WriteAllLines($winSumsPath, $winSums, (New-Object System.Text.UTF8Encoding($false)))

    $winZip = Join-Path $distDir "hex-$Version-windows-x64.zip"
    if (Test-Path $winZip) {
        Remove-Item $winZip -Force
    }
    Compress-Archive -Path "$winStaging\*" -DestinationPath $winZip -Force
    Write-Host "Windows package created: $winZip" -ForegroundColor Green
} finally {
    Remove-Item $winStaging -Recurse -Force -ErrorAction SilentlyContinue
}

# Step 2: macOS build over Tailscale
Write-Host ""
Write-Host "==> [2/3] Building macOS ARM64 release on $MacHost..." -ForegroundColor Cyan

$remoteScriptPath = "/tmp/hex-release-$Version.sh"
$localTempScript = [System.IO.Path]::GetTempFileName()

$bashLines = @(
    '#!/usr/bin/env bash',
    'set -euo pipefail',
    "cd $MacRepo",
    'if [ ! -s secrets/antigravity-client-secret.txt ]; then',
    '    echo "Error: secrets/antigravity-client-secret.txt is missing or empty" >&2',
    '    exit 1',
    'fi',
    'if [ -n "$(git status --porcelain --untracked-files=no)" ]; then',
    '    echo "Error: the Mac checkout has uncommitted changes" >&2',
    '    exit 1',
    'fi',
    'previous=$(git symbolic-ref --short -q HEAD || git rev-parse HEAD)',
    'trap ''git checkout -q "$previous" || true'' EXIT',
    'git fetch origin',
    "git checkout -q `"$commitSha`"",
    'export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer',
    "export HEX_VERSION=`"$Version`"",
    'export HEX_RELEASE_UNSIGNED=1',
    '# A fork build must never replace itself with an upstream release.',
    'export HEX_LOCAL_BUILD=1',
    './scripts/build-app.sh',
    'mkdir -p dist',
    "zip_file=`"dist/hex-${Version}-macos-arm64.zip`"",
    "dmg_file=`"dist/hex-${Version}-macos-arm64.dmg`"",
    'rm -f "$zip_file"',
    '/usr/bin/ditto -c -k --keepParent target/app/Hex.app "$zip_file"',
    'rm -f "$dmg_file"',
    'if [ -x ./scripts/package-dmg.sh ]; then',
    '    ./scripts/package-dmg.sh target/app/Hex.app "$dmg_file"',
    'elif command -v hdiutil >/dev/null 2>&1; then',
    '    staging=$(mktemp -d "${TMPDIR:-/tmp}/hex-dmg.XXXXXX")',
    '    /usr/bin/ditto target/app/Hex.app "$staging/Hex.app"',
    '    ln -s /Applications "$staging/Applications"',
    '    if [ -f packaging/macos/dmg/background.tiff ]; then',
    '        cp packaging/macos/dmg/background.tiff "$staging/.background.tiff"',
    '    fi',
    '    if [ -f packaging/macos/dmg/DS_Store ]; then',
    '        cp packaging/macos/dmg/DS_Store "$staging/.DS_Store"',
    '    fi',
    '    for attempt in 1 2 3 4 5; do',
    '        hdiutil create -quiet -fs APFS -format ULFO -volname Hex -srcfolder "$staging" "$dmg_file" && break',
    '        echo "hdiutil create failed (attempt $attempt); retrying in 2s..." >&2',
    '        rm -f "$dmg_file"',
    '        sleep 2',
    '    done',
    '    rm -rf "$staging"',
    'fi',
    'echo "macOS build and packaging complete."',
    'ls -lh dist/hex-*'
)

$bashScriptText = ($bashLines -join "`n") + "`n"
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($localTempScript, $bashScriptText, $utf8NoBom)

try {
    Write-Host "Copying build script to ${MacHost}:$remoteScriptPath..."
    & scp $localTempScript "${MacHost}:$remoteScriptPath"
    if ($LASTEXITCODE -ne 0) {
        throw "scp of build script failed with exit code $LASTEXITCODE"
    }

    Write-Host "Running remote build script on $MacHost..."
    & ssh -n $MacHost "bash $remoteScriptPath"
    $remoteStatus = $LASTEXITCODE
    & ssh -n $MacHost "rm -f $remoteScriptPath" 2>$null
    if ($remoteStatus -ne 0) {
        throw "Remote build failed on $MacHost with exit code $remoteStatus"
    }

    Write-Host "Downloading macOS artifacts from $MacHost..."
    $macZip = Join-Path $distDir "hex-$Version-macos-arm64.zip"
    & scp "${MacHost}:$MacRepo/dist/hex-$Version-macos-arm64.zip" $macZip
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to download macOS zip artifact from $MacHost"
    }

    $macDmg = Join-Path $distDir "hex-$Version-macos-arm64.dmg"
    & scp "${MacHost}:$MacRepo/dist/hex-$Version-macos-arm64.dmg" $macDmg 2>$null
    if (Test-Path $macDmg) {
        Write-Host "Downloaded macOS DMG: $macDmg" -ForegroundColor Green
    }
} finally {
    Remove-Item $localTempScript -Force -ErrorAction SilentlyContinue
}

# Step 3: Publish to GitHub
Write-Host ""
Write-Host "==> [3/3] Publishing release $tagName to GitHub..." -ForegroundColor Cyan

$distArtifacts = Get-ChildItem -Path $distDir -File | Where-Object { $_.Name -ne 'SHA256SUMS' }
$checksumLines = @()
foreach ($artifact in $distArtifacts) {
    $sha = (Get-FileHash -Path $artifact.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksumLines += "$sha  $($artifact.Name)"
}
$sumsFile = Join-Path $distDir 'SHA256SUMS'
[System.IO.File]::WriteAllLines($sumsFile, $checksumLines, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Generated $sumsFile"

$notesTempFile = [System.IO.Path]::GetTempFileName()
[System.IO.File]::WriteAllText($notesTempFile, $notesContent, (New-Object System.Text.UTF8Encoding($false)))

try {
    $allArtifacts = Get-ChildItem -Path $distDir -File | ForEach-Object { $_.FullName }
    Write-Host "Creating GitHub Release $tagName on $Repo..."
    & gh release create $tagName `
        --repo $Repo `
        --target $commitSha `
        --title "Hex $Version" `
        --notes-file $notesTempFile `
        @allArtifacts

    if ($LASTEXITCODE -ne 0) {
        throw "gh release create failed with exit code $LASTEXITCODE"
    }
    Write-Host "Successfully published Hex $Version release to GitHub!" -ForegroundColor Green
} finally {
    Remove-Item $notesTempFile -Force -ErrorAction SilentlyContinue
}
