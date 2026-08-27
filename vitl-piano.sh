#!/usr/bin/env bash
DIR="$HOME/.local/share/vitl-piano"
export LD_LIBRARY_PATH="$DIR/lib:$LD_LIBRARY_PATH"
rm -rf "$HOME/.cache/vitl-piano-desktop" "$HOME/.cache/vitl_piano"* 2>/dev/null || true
exec "$DIR/vitl-piano-desktop" "$@"
