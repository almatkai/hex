#!/bin/bash
set -euo pipefail

# Ensure Xcode toolchain is selected for Apple Intelligence Swift macros
if [ -d "/Applications/Xcode.app/Contents/Developer" ]; then
  export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Run hex desktop app with passed arguments
exec cargo run -- app "$@"
