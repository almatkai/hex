# Releasing the fork (local builds, one-click updates)

The fork (`almatkai/hex`) is built on local hardware only (Windows here, macOS
arm64 over Tailscale; never GitHub Actions) and published to its own GitHub
Releases. Installed copies of Hex read the same releases and update
themselves: the window shows the update, downloads it in the background and
offers **Restart to update**. Anomaly's release service is never contacted.

```ts
scripts/release-local.ps1 -Version 3.4.1
├── build   Windows hex.exe + hex.com here, Hex.app (zip + dmg) on the Mac
├── sign    every file with secrets/update-signing.key → <file>.sig (ed25519)
├── publish release v3.4.1: files, .sig files, stable.json, install-hex.ps1
└── move    prerelease channel-stable: stable.json (the feed Hex reads)
Installed Hex (src/updater/fork.rs)
├── every hour + Check for updates: GET releases/download/channel-stable/stable.json
├── new build? download it, check size + SHA-256 + the .sig against fork::KEY
└── Restart to update → the new version runs; settings, history, models kept
```

## How updates are trusted

- The manifest is the Anomaly release protocol's schema 1
  (`src/updater/protocol.rs`), with every file under the immutable
  `v<version>` release on github.com.
- Every target has a `<target>-sig`: the fork's ed25519 key signing
  `almatkai.hex.update.v1\n<sha256>\n`. Hex refuses a file whose signature
  doesn't verify against `KEY` in `src/updater/fork.rs`, on every OS, before
  it runs. Whoever could change a release still can't make Hex run their file.
- Windows needs no Authenticode and macOS no Developer ID: the fork has
  neither. macOS still requires an intact ad-hoc signature named
  `anomaly.hex`.
- The private key is `secrets/update-signing.key` (git-ignored). **Back it up
  somewhere safe and never commit it.** Losing it strands every installed copy
  on its current version; whoever has it can push updates to every user.
  `hex release-keygen --key <path>` made it once; it refuses to overwrite.

## Installing for the first time

- **Windows:** in PowerShell,
  `irm https://github.com/almatkai/hex/releases/latest/download/install-hex.ps1 | iex`.
  It installs into `%APPDATA%\hex\versions\<version>` with the receipt
  (`install.json`), Start Menu shortcut and `hex` on PATH, then starts Hex.
  `HEX_INSTALL_CHANNEL=beta` picks another channel, `HEX_UNINSTALL=1` removes
  it (settings stay).
- **macOS:** open the `.dmg` and drag Hex to Applications. A copy in a folder
  you can write to updates itself.
- Copies built from a checkout (`HEX_LOCAL_BUILD=1`, `HEX_TEST_BUILD=1`) never
  update; install a release once to start receiving updates.

## Prerequisites

1. On `dev`, clean, equal to `origin/dev`.
2. `$env:GH_TOKEN` with write access to the repository (never printed).
3. `secrets/update-signing.key` and `secrets/antigravity-client-secret.txt`
   here, and the Antigravity secret in the Mac checkout's `secrets/`.
4. The Mac reachable over Tailscale (`almat@100.67.107.38`, checkout
   `~/Desktop/code/hex`, clean), with Xcode.

## Usage

```powershell
scripts\release-local.ps1 -Version 3.4.1 -DryRun            # checks + plan only
scripts\release-local.ps1 -Version 3.4.1                    # stable
scripts\release-local.ps1 -Version 3.5.0-beta.1 -Channel beta
scripts\release-local.ps1 -Version 3.4.2 -SkipMac           # Windows only
```

Use a new version for every release; never replace published files. A
channel's pointer only moves after its versioned release has every file.
Versions are never compared: whatever the channel's pointer names is the
build installed copies take.
