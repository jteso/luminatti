#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
output="${1:-$root_dir/target/debug/luminatti-radar-layout}"
mkdir -p "$(dirname -- "$output")"
output="$(cd -- "$(dirname -- "$output")" && pwd)/$(basename -- "$output")"
cd "$root_dir/tools/radar-layout"
go build -mod=readonly -trimpath -o "$output" .
