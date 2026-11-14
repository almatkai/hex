# Releasing Fork (Local Builds)

Hex fork releases (`almatkai/hex`) are built completely from local hardware—Windows locally and macOS arm64 over Tailscale—in strict adherence to the project policy forbidding GitHub Actions build workflows.

The script `scripts/release-local.ps1` automates building, packaging, and publishing release artifacts.

## Prerequisites

1. **Branch & State**:
   - Current branch must be `dev`.
   - Working tree must be clean (no uncommitted or untracked changes).
   - Local branch must be up to date with `origin/dev`.
2. **GitHub Authentication**:
   - `$env:GH_TOKEN` must be set in the environment (never printed or checked into git). GitHub CLI (`gh`) uses this token to publish releases.
3. **Antigravity Client Secret**:
   - `secrets/antigravity-client-secret.txt` must exist and be non-empty on both:
     - Local machine: `<repo_root>/secrets/antigravity-client-secret.txt`
     - Remote machine: `~/Desktop/code/hex/secrets/antigravity-client-secret.txt`
4. **Tailscale & Remote macOS**:
   - macOS host reachable via SSH (default: `almat@100.67.107.38`).
   - Repository at `~/Desktop/code/hex` on the macOS host.
   - Xcode and command-line tools installed at `/Applications/Xcode.app/Contents/Developer`.
5. **Toolchains**:
   - Windows: Rust MSVC toolchain (`x86_64-pc-windows-msvc`), Vulkan SDK, `gh` CLI.
   - macOS: Rust toolchain, Xcode developer tools.

## Usage

### 1. Dry Run (Precondition Check & Plan Preview)

Verify all preconditions and preview the release execution plan without performing remote compilation or publishing to GitHub:

```powershell
powershell -NoProfile -File scripts/release-local.ps1 -Version 3.4.1 -DryRun
```

### 2. Live Release

When ready to build and publish:

```powershell
$env:GH_TOKEN = "your_github_token"
powershell -NoProfile -File scripts/release-local.ps1 -Version 3.4.1
```

### Optional Parameters

- `-MacHost <string>`: Tailscale SSH host (default: `almat@100.67.107.38`).
- `-MacRepo <string>`: Remote repository directory (default: `~/Desktop/code/hex`).
- `-Repo <string>`: Target GitHub repository (default: `almatkai/hex`).
- `-AllowAnyBranch`: Allow running from a branch other than `dev` (useful for testing).

## Release Process Details

1. **Preconditions**:
   - Checks branch is `dev`, tree is clean, and commit is synchronized with `origin/dev`.
   - Verifies `$env:GH_TOKEN` is set.
   - Verifies client secret files exist on both machines.
   - Checks that release tag `v<Version>` does not already exist.
2. **Windows Build**:
   - Builds `hex.exe` (GUI application) and `hex-console` (installed beside it as `hex.com`).
   - Environment: `TRANSCRIBE_CMAKE_ARGS='-DGGML_NATIVE=OFF'`, `HEX_VERSION=<Version>`, `HEX_LOCAL_BUILD=1`. A fork build keeps its updater off: otherwise it would replace itself with the upstream (Anomaly) release.
   - Packages `dist/<Version>/hex-<Version>-windows-x64.zip` containing `hex.exe`, `hex.com`, and package `SHA256SUMS`.
3. **macOS Build**:
   - Generates LF/no-BOM shell script and transfers to remote macOS host over Tailscale via `scp`.
   - Refuses a Mac checkout with uncommitted changes, checks out the exact same commit SHA, and switches back to the previous branch when done.
   - Builds ad-hoc signed `Hex.app` with `HEX_RELEASE_UNSIGNED=1`, `HEX_LOCAL_BUILD=1` (updates off) and `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`.
   - Zips `Hex.app` using `ditto -c -k --keepParent` into `hex-<Version>-macos-arm64.zip`.
   - Packages `hex-<Version>-macos-arm64.dmg` using `hdiutil` and repo styling.
   - Copies artifacts back to local `dist/<Version>/`.
4. **Publish**:
   - Generates `dist/<Version>/SHA256SUMS` covering this release's Windows and macOS artifacts only.
   - Generates markdown changelog from `git log` since the previous release tag.
   - Publishes GitHub Release `v<Version>` with all artifacts and changelog via `gh release create`.
