#!/usr/bin/env bash
# Replaces an existing user-owned Luminatti.app after the running process
# exits. This deliberately uses only macOS system tools so preview builds do
# not need an Apple Developer account or a separate updater binary.
set -euo pipefail

archive_url="${1:?archive URL is required}"
checksum_url="${2:?checksum URL is required}"
app_bundle="${3:?app bundle path is required}"
parent_pid="${4:?parent PID is required}"

case "$archive_url" in
  https://github.com/jteso/luminatti/releases/download/*) ;;
  *) echo "Refusing an update from an unexpected host." >&2; exit 2 ;;
esac

case "$checksum_url" in
  https://github.com/jteso/luminatti/releases/download/*) ;;
  *) echo "Refusing a checksum from an unexpected host." >&2; exit 2 ;;
esac

if [[ "${app_bundle##*/}" != "Luminatti.app" ]]; then
  echo "Unexpected app bundle: $app_bundle" >&2
  exit 2
fi

app_parent="$(dirname "$app_bundle")"
if [[ ! -w "$app_parent" ]]; then
  echo "Cannot update $app_bundle because its parent folder is not writable." >&2
  exit 3
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/luminatti-update.XXXXXX")"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

archive="$work_dir/Luminatti.zip"
checksum_file="$work_dir/Luminatti.zip.sha256"
/usr/bin/curl --fail --location --silent --show-error "$archive_url" --output "$archive"
/usr/bin/curl --fail --location --silent --show-error "$checksum_url" --output "$checksum_file"

expected_checksum="$(/usr/bin/awk 'NF { print $1; exit }' "$checksum_file")"
actual_checksum="$(/usr/bin/shasum -a 256 "$archive" | /usr/bin/awk '{ print $1 }')"
if [[ ! "$expected_checksum" =~ ^[A-Fa-f0-9]{64}$ ]] || [[ "$expected_checksum" != "$actual_checksum" ]]; then
  echo "The downloaded update did not match its published SHA-256 checksum." >&2
  exit 4
fi

/usr/bin/ditto -x -k "$archive" "$work_dir/unpacked"
replacement="$work_dir/unpacked/Luminatti.app"
if [[ ! -x "$replacement/Contents/MacOS/Luminatti" ]]; then
  echo "The update archive does not contain a valid Luminatti.app." >&2
  exit 5
fi

# The parent application starts this helper and immediately quits. Do not move
# the bundle until macOS has released the executable and its resources.
while /bin/kill -0 "$parent_pid" 2>/dev/null; do
  /bin/sleep 0.1
done

staged_bundle="$app_parent/.Luminatti.app.staged"
backup_bundle="$app_parent/.Luminatti.app.previous"
/bin/rm -rf "$staged_bundle" "$backup_bundle"
/usr/bin/ditto "$replacement" "$staged_bundle"
/bin/mv "$app_bundle" "$backup_bundle"
/bin/mv "$staged_bundle" "$app_bundle"
/usr/bin/open -a "$app_bundle"
/bin/rm -rf "$backup_bundle"
