# Install Hex (the almatkai/hex fork) for the current user as a managed,
# self-updating Windows application, from the fork's GitHub Releases:
#
#   irm https://github.com/almatkai/hex/releases/latest/download/install-hex.ps1 | iex
#
# The app lands in %APPDATA%\hex\versions\<version>\hex.exe with hex.com
# beside it, a Start Menu shortcut points at hex.exe, %APPDATA%\hex\bin (a
# copy of hex.com) goes on PATH, and install.json marks the install as
# direct, so Hex then updates itself from the same feed with one click
# (src/updater/fork.rs). Settings, history and models are kept.
#
# Trust: HTTPS to GitHub and the manifest's size and SHA-256 for every file.
# The app also checks the fork's ed25519 signature on every update.
#
# Environment (no parameters, so it runs under `iex`):
#   HEX_INSTALL_CHANNEL=stable|beta|dev   the update channel (default stable)
#   HEX_UNINSTALL=1                       remove Hex, keep settings and logs
#   HEX_NO_LAUNCH=1                       don't start Hex afterwards

& {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$origin = 'https://github.com/almatkai/hex/releases/download'
$channel = if ($env:HEX_INSTALL_CHANNEL) { $env:HEX_INSTALL_CHANNEL } else { 'stable' }
if ($channel -notin @('stable', 'beta', 'dev')) { throw 'HEX_INSTALL_CHANNEL must be stable, beta or dev.' }
$support = Join-Path $env:APPDATA 'hex'
$versions = Join-Path $support 'versions'
$pointer = Join-Path $support 'current-version'
$receipt = Join-Path $support 'install.json'
$bin = Join-Path $support 'bin'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Hex.lnk'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$managedMark = 'Hex managed install'

function Remove-ManagedShortcut {
    if (Test-Path $shortcut) {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        if ($link.Description -eq $managedMark) {
            Remove-Item $shortcut -Force
        }
    }
}

# The user's PATH without (or with, once) bin, so a terminal runs `hex` as
# hex.com, the console program that waits for hex.exe.
function Set-BinOnPath([bool]$present) {
    $environment = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    $current = [string]$environment.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $entries = @($current -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ne $bin })
    if ($present) { $entries += $bin }
    $updated = $entries -join ';'
    if ($updated -ne $current) {
        $environment.SetValue('Path', $updated, [Microsoft.Win32.RegistryValueKind]::ExpandString)
        # SetEnvironmentVariable broadcasts the change to running programs.
        [Environment]::SetEnvironmentVariable('HexPathRefresh', '1', 'User')
        [Environment]::SetEnvironmentVariable('HexPathRefresh', $null, 'User')
    }
    $environment.Close()
}

if ($env:HEX_UNINSTALL -eq '1') {
    Remove-ManagedShortcut
    Set-BinOnPath $false
    if (Test-Path $bin) { Remove-Item $bin -Recurse -Force }
    try { Remove-ItemProperty -Path $runKey -Name 'Hex' -ErrorAction Stop } catch {}
    if (Test-Path $versions) { Remove-Item $versions -Recurse -Force }
    if (Test-Path $pointer) { Remove-Item $pointer -Force }
    if (Test-Path $receipt) { Remove-Item $receipt -Force }
    Write-Output "Removed Hex. Logs and settings remain in $support."
    return
}

# A 32-bit PowerShell on 64-bit Windows reports the real machine here.
$machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($machine -ne 'AMD64') { throw "This Hex build supports 64-bit x64 Windows, not $machine." }
$target = 'windows-x64'

$manifestUrl = "$origin/channel-$channel/$channel.json"
Write-Output "Fetching $manifestUrl..."
# GitHub serves release assets as application/octet-stream, which Windows
# PowerShell hands back as bytes.
$response = Invoke-WebRequest -Uri $manifestUrl -UseBasicParsing -TimeoutSec 60
$text = if ($response.Content -is [byte[]]) { [Text.Encoding]::UTF8.GetString($response.Content) } else { $response.Content }
$manifest = $text | ConvertFrom-Json
if ($manifest.schema -ne 1) { throw "Unsupported manifest schema $($manifest.schema)." }
if ($manifest.app -ne 'hex' -or $manifest.artifact -ne 'desktop' -or $manifest.channel -ne $channel) {
    throw "The manifest is not Hex desktop on the $channel channel."
}
$version = $manifest.release.version
# Display text that names a directory; versions are never compared.
if ($version -notmatch '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$') { throw "Unexpected version $version." }
$file = $manifest.release.targets.$target
if (-not $file) { throw "Hex $version has no $target build." }

function Resolve-Download($entry, [string]$name) {
    if ($entry.sha256 -notmatch '^[0-9a-f]{64}$' -or $entry.size -le 0) { throw "The manifest's $name entry is invalid." }
    $url = [Uri]::new([Uri]$manifestUrl, $entry.url).AbsoluteUri
    if (-not $url.StartsWith("$origin/")) { throw "The manifest points outside $origin." }
    $url
}

# Moves <source> over <path>. A running program can't be replaced but can be
# renamed, so one in the way moves aside to <path>.old, removed next time.
function Replace-File([string]$source, [string]$path) {
    $aside = "$path.old"
    if (Test-Path $aside) { Remove-Item $aside -Force -ErrorAction SilentlyContinue }
    try {
        Move-Item $source $path -Force
    } catch {
        Move-Item $path $aside -Force
        Move-Item $source $path -Force
    }
}

# Downloads to <path>.partial, checks size and SHA-256 against the manifest,
# then moves it into place.
function Save-Download($entry, [string]$url, [string]$path) {
    $partial = "$path.partial"
    if (Test-Path $partial) { Remove-Item $partial -Force }
    Invoke-WebRequest -Uri $url -OutFile $partial -UseBasicParsing -TimeoutSec 900
    $bytes = (Get-Item $partial).Length
    if ($bytes -ne $entry.size) {
        Remove-Item $partial -Force
        throw "The download has $bytes bytes, expected $($entry.size)."
    }
    $sha = (Get-FileHash -Algorithm SHA256 $partial).Hash.ToLowerInvariant()
    if ($sha -ne $entry.sha256) {
        Remove-Item $partial -Force
        throw 'The download failed checksum verification.'
    }
    Replace-File $partial $path
}

$fileUrl = Resolve-Download $file $target
$consoleFile = $manifest.release.targets."$target-cli"
if (-not $consoleFile) { throw "Hex $version has no $target-cli build." }
$consoleUrl = Resolve-Download $consoleFile "$target-cli"

$versionDir = Join-Path $versions $version
$executable = Join-Path $versionDir 'hex.exe'
$console = Join-Path $versionDir 'hex.com'
New-Item -ItemType Directory -Force $versionDir | Out-Null
Write-Output "Downloading Hex $version (this takes a minute)..."
Save-Download $file $fileUrl $executable
Save-Download $consoleFile $consoleUrl $console
# hex.com waits for the hex.exe beside it and prints its version.
$reported = (& $console --version) -split '\s+' | Select-Object -Last 1
if ($reported -ne $version) {
    Remove-Item $executable -Force
    Remove-Item $console -Force
    throw "The downloaded Hex reports version $reported, expected $version."
}

$pointerPartial = Join-Path $support 'current-version.partial'
Set-Content -Path $pointerPartial -Value $version -NoNewline
Move-Item $pointerPartial $pointer -Force
$receiptPartial = "$receipt.partial"
# WriteAllText writes UTF-8 without a byte order mark on every PowerShell.
[IO.File]::WriteAllText($receiptPartial, ([ordered]@{ app = 'hex'; artifact = 'desktop'; channel = $channel; method = 'direct'; root = $support } | ConvertTo-Json -Compress))
Move-Item $receiptPartial $receipt -Force

$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $executable
$link.Arguments = 'app'
$link.Description = $managedMark
$link.WorkingDirectory = $versionDir
$link.Save()

New-Item -ItemType Directory -Force $bin | Out-Null
$copy = Join-Path $bin 'hex.com.partial'
Copy-Item $console $copy -Force
Replace-File $copy (Join-Path $bin 'hex.com')
Set-BinOnPath $true

Write-Output "Installed Hex $version. It updates itself: when a new version is ready, Hex shows Restart to update."
Write-Output "  Start Menu: Hex"
Write-Output "  Terminal: hex (open a new terminal)"
if ($env:HEX_NO_LAUNCH -ne '1') {
    # A Hex already running quits first, so the new one takes over.
    if (Get-Process hex -ErrorAction SilentlyContinue) {
        & $console quit | Out-Null
        Start-Sleep -Seconds 2
    }
    Start-Process $executable -ArgumentList 'app'
}
}
