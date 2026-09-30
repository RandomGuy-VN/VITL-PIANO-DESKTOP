#!/usr/bin/env bash
# Creates the v1.0-beta.3 pre-release and uploads the Linux assets.
#
# Run this from a clone of the repo where `gh` is authenticated:
#   gh auth status        # must be logged in with repo write access
#   bash create-release-v1.0-beta.3.sh
#
# The Claude Code web session that built Beta 3 is blocked from creating
# releases and from pushing tags, so this has to run from your own machine.
set -euo pipefail

REPO="RandomGuy-VN/VITL-PIANO-DESKTOP"
TAG="v1.0-beta.3"
TARGET="869b2b9f6ed15468bdf60ee220dfcbaf1729a4f5"   # main, "Merge pull request #2"
NOTES="RELEASE_NOTES_v1.0-beta.3.md"

cd "$(dirname "$0")"

for f in "$NOTES" vitl-piano-linux.zip vitl-piano-desktop desktop.html web/install.sh; do
  [ -f "$f" ] || { echo "missing: $f — run this from the repo root on main" >&2; exit 1; }
done

# The assets must be the ones built for this tag, not a stale checkout.
SUMS='0353ba6b1364977e97d996fab534fc988511bbbcef943d91cbe8aac78ad91c9b  vitl-piano-linux.zip
73d2a22f341093c83a11801eb32e13243c30412d54558a48c0a4be1f7df721be  vitl-piano-desktop
42832a5096486bb79a65c933317c3f93076ac6a90c86e5a9b61ee6645cf50492  desktop.html
f044dccde477ca08a5d33e92a8e0c11f33410d8876c0d8d277c345873e8caf52  web/install.sh'

if command -v sha256sum >/dev/null 2>&1; then
  echo "Verifying asset checksums…"
  printf '%s\n' "$SUMS" | sha256sum -c --strict -
elif command -v shasum >/dev/null 2>&1; then
  echo "Verifying asset checksums…"
  printf '%s\n' "$SUMS" | shasum -a 256 -c --strict -
else
  echo "No sha256sum/shasum found — skipping the checksum check." >&2
fi

# install.sh has to land under its own name, not web/install.sh.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp web/install.sh "$TMP/install.sh"

echo "Creating $TAG as a pre-release on $TARGET…"
gh release create "$TAG" \
  --repo "$REPO" \
  --target "$TARGET" \
  --title "VITL Piano Desktop V1.0 Public Beta 3" \
  --notes-file "$NOTES" \
  --prerelease \
  vitl-piano-linux.zip \
  vitl-piano-desktop \
  desktop.html \
  "$TMP/install.sh"

echo
echo "Done. Checking that /releases/latest still points at Beta 2 (so Windows keeps working):"
gh api "repos/$REPO/releases/latest" --jq '.tag_name'
