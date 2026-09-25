#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
app_dir="$root_dir/target/Luminatti.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
binary_path=""
create_zip=false

usage() {
  echo "Usage: $0 [--binary PATH] [--zip]" >&2
  echo "  --binary PATH  Package an already-built desktop binary." >&2
  echo "  --zip          Create target/Luminatti-macos-<arch>.zip." >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)
      binary_path="${2:?--binary needs a path}"
      shift 2
      ;;
    --zip)
      create_zip=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

cd "$root_dir"
if [[ -z "$binary_path" ]]; then
  cargo build --release --features desktop
  binary_path="$root_dir/target/release/luminatti"
fi
if [[ ! -x "$binary_path" ]]; then
  echo "Desktop binary not found or not executable: $binary_path" >&2
  exit 1
fi

radar_layout_path="$root_dir/target/release/luminatti-radar-layout"
bash "$root_dir/scripts/build-radar-layout.sh" "$radar_layout_path"
version="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -1)"
if [[ -z "$version" ]]; then
  echo "Could not read a version from Cargo.toml" >&2
  exit 1
fi

rm -rf "$app_dir"
mkdir -p "$macos_dir"
cp "$binary_path" "$macos_dir/Luminatti"
# Older installed updaters expect the lowercase path in replacement archives.
# On the usual case-insensitive filesystem, the uppercase file already matches.
if [[ ! -e "$macos_dir/luminatti" ]]; then
  ln -s Luminatti "$macos_dir/luminatti"
fi
cp "$radar_layout_path" "$macos_dir/luminatti-radar-layout"
mkdir -p "$contents_dir/Resources"
cp "$root_dir/tools/radar-layout/NOTICE.md" "$contents_dir/Resources/Radar-layout-NOTICE.md"
cp "$root_dir/tools/radar-layout/D2-LICENSE.txt" "$contents_dir/Resources/D2-LICENSE.txt"
cp "$root_dir/scripts/install-macos-update.sh" "$contents_dir/Resources/install-macos-update.sh"
chmod +x "$contents_dir/Resources/install-macos-update.sh"

cat > "$contents_dir/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleDisplayName</key><string>Luminatti</string>
  <key>CFBundleExecutable</key><string>launch</string>
  <key>CFBundleIdentifier</key><string>io.github.jteso.luminatti</string>
  <key>CFBundleName</key><string>Luminatti</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
</dict></plist>
PLIST

cat > "$macos_dir/launch" <<'LAUNCH'
#!/usr/bin/env bash
# Preserve a repository passed by a terminal. Finder has no useful working
# directory, so make the first-run experience a folder picker instead.
if ! /usr/bin/git -C "$PWD" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  selected="$(/usr/bin/osascript -e 'POSIX path of (choose folder with prompt "Choose a Git repository to review")')" || exit 0
  cd "$selected"
fi
exec "$(dirname "$0")/Luminatti" desktop
LAUNCH
chmod +x "$macos_dir/launch"

echo "Built $app_dir"
if [[ "$create_zip" == true ]]; then
  case "$(uname -m)" in
    arm64) architecture="arm64" ;;
    x86_64) architecture="x64" ;;
    *) echo "Unsupported macOS architecture: $(uname -m)" >&2; exit 1 ;;
  esac
  archive="$root_dir/target/Luminatti-macos-$architecture.zip"
  rm -f "$archive" "$archive.sha256"
  /usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app_dir" "$archive"
  /usr/bin/shasum -a 256 "$archive" > "$archive.sha256"
  echo "Packaged $archive"
fi
