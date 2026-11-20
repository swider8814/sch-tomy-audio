#!/usr/bin/env bash
# Generates all platform icons from src-tauri/icons/source.svg with the
# Tauri CLI. Run from the repository root after `npm ci`.
# Only the files listed in tauri.conf.json (plus icon.png) are kept; the
# CLI also writes Android, iOS and Windows Store icons the app does not use.
set -euo pipefail
src=src-tauri/icons/source.svg
dst=src-tauri/icons
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
npx tauri icon "$src" -o "$tmp"
for f in 32x32.png 128x128.png 128x128@2x.png icon.icns icon.ico icon.png; do
  cp "$tmp/$f" "$dst/$f"
done
