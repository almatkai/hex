# Claude Instructions for Hex

## Strict Build & CI Policy (CRITICAL)

- **NEVER RUN BUILDS IN GITHUB ACTIONS**: Do NOT trigger or dispatch build workflows (`gh workflow run`, `Test builds`, `Release`, etc.) on GitHub Actions. Runner minutes, quota, and GitHub tokens are strictly limited and reserved for other projects.
- **Local Builds Only**: All application builds must be performed locally on the current machine (`cargo build`, `scripts/build-app.sh`, etc.).
- **Builds for Another Device / OS**:
  - If a build is requested for another device or operating system (e.g., requested a macOS/MacBook build while running on Windows):
    - **DO NOT USE GITHUB ACTIONS**.
    - Explicitly inform the user that the target is a different physical machine/OS.
    - Ask the user whether to attempt connecting and building via **Tailscale**, or provide the exact commands for the user to execute locally on that machine.

## Repository & Development Rules

- **Branching**: All work branches from `dev` and merges back into `dev` via PRs. There is no `main`.
- **Labs Gate**: New or experimental features belong behind Labs flags, off by default.
- **Build Commands**:
  - **Windows**: `cargo build --release --bin hex`
  - **macOS**: `./scripts/setup.sh` followed by `HEX_RELEASE_UNSIGNED=1 HEX_LOCAL_BUILD=1 ./scripts/build-app.sh` (the real `Hex.app`, `anomaly.hex`, data in `~/Library/Application Support/hex`, ad-hoc signed, updates off). `HEX_TEST_BUILD=1` makes `Hex Test.app` with its own `hex-test` data folder; use it only when a separate test app is wanted.
  - **Linux**: `cargo build --release --bin hex`
