---
name: hex-local-run
description: Build, configure, and run Hex locally on macOS, including enabling Labs features like Antigravity meeting summaries. Use when running Hex from source or troubleshooting macOS builds.
license: MIT
---

# Hex Local Run & Dev Workflow (macOS)

This skill guides local building, running, and configuring Hex on macOS.

## 1. Prerequisites (macOS Toolchain)
Hex builds Swift bridges (`apple_intelligence.swift`, `apple_speech.swift`). On macOS, Swift macros require Xcode's toolchain rather than bare CommandLineTools:
```bash
export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
```

## 2. Sync with Latest `dev`
Hex development is based on the `dev` branch:
```bash
git checkout dev
git pull origin dev
```

## 3. Running Hex Locally
Run directly via the helper script:
```bash
./scripts/run-local.sh
```
Or directly via cargo:
```bash
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo run -- app
```

## 4. Enabling Antigravity Meeting Summaries (Labs)
Antigravity meeting summaries (`labs.antigravity_summaries`) is a Labs feature and is turned off by default.
To enable it:
```bash
# Using hex CLI
./target/debug/hex labs on antigravity-summaries
```
Once enabled, "Antigravity" appears under:
**Settings > Meetings > After a meeting > Summaries (Сводки)**.

## 5. Installing as Local Application (/Applications/Hex.app)
To install the build into `/Applications/Hex.app`:
```bash
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer HEX_LOCAL_BUILD=1 HEX_RELEASE_UNSIGNED=1 ./scripts/install-app.sh
```
