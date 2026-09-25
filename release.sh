#!/usr/bin/env bash
# Prepare a version on main, then push its tag to start the release workflow.
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$root_dir"

usage() {
  echo "Usage: ./release.sh <major.minor.patch>" >&2
  echo "Commits the version on main, pushes main, then pushes v<version>." >&2
}

if [[ $# -ne 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  usage
  if [[ $# -eq 1 && ( "$1" == "-h" || "$1" == "--help" ) ]]; then exit 0; fi
  exit 2
fi

version="$1"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Expected a version such as 2.33.0." >&2
  exit 2
fi
tag="v$version"

if [[ "$(git branch --show-current)" != "main" ]]; then
  echo "Run this script from the main branch." >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "Commit or remove working tree changes before preparing a release." >&2
  exit 1
fi
if ! command -v cargo >/dev/null 2>&1 || ! command -v python3 >/dev/null 2>&1; then
  echo "cargo and python3 are required." >&2
  exit 1
fi

git fetch origin main
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
  echo "Local main must match origin/main before preparing a release." >&2
  exit 1
fi
if git show-ref --verify --quiet "refs/tags/$tag" ||
   [[ -n "$(git ls-remote --tags origin "refs/tags/$tag")" ]]; then
  echo "Tag $tag already exists." >&2
  exit 1
fi

current_version="$(sed -nE '/^\[package\]$/,/^\[/{s/^version = "([^"]+)"/\1/p;}' Cargo.toml | head -n 1)"
if [[ "$version" == "$current_version" ]]; then
  echo "Cargo.toml already has version $version." >&2
  exit 1
fi

python3 - "$version" <<'PY'
from pathlib import Path
import re
import sys

path = Path("Cargo.toml")
manifest = path.read_text()
updated, count = re.subn(
    r'(?m)^(\[package\]\n(?:(?!\[).)*?version = ")[^"]+("$)',
    lambda match: f"{match.group(1)}{sys.argv[1]}{match.group(2)}",
    manifest,
    count=1,
    flags=re.DOTALL,
)
if count != 1:
    raise SystemExit("Could not find the package version in Cargo.toml")
path.write_text(updated)
PY
cargo update --workspace

git add Cargo.toml Cargo.lock
git commit -m "chore: release $tag"
git tag -a "$tag" -m "Release $tag"
git push origin main
git push origin "refs/tags/$tag"

echo "Pushed $tag. GitHub Actions will build and publish the release."
