#!/usr/bin/env bash
# Run the native desktop app from this checkout for local development.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir"

cargo build --features desktop
# macOS uses the executable name for an unbundled app's process identity.
# Keep Cargo's lowercase binary for the CLI, and launch the desktop build
# through a capitalized hard link in a separate directory. The two names
# cannot coexist in target/debug on a case-insensitive macOS filesystem.
desktop_dir="target/debug/luminatti-desktop"
mkdir -p "$desktop_dir"
rm -f "$desktop_dir/Luminatti"
ln target/debug/luminatti "$desktop_dir/Luminatti"
exec "$desktop_dir/Luminatti" desktop "$@"
