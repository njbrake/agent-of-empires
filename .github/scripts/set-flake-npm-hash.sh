#!/usr/bin/env bash
# Validates the hash shape so an injected value cannot pollute the sed
# replacement; errors if `flake.nix` does not have exactly one match.

set -euo pipefail

FLAKE="${1:?usage: $0 <path-to-flake.nix> <sha256-hash>}"
NEW_HASH="${2:?usage: $0 <path-to-flake.nix> <sha256-hash>}"

if [ ! -f "$FLAKE" ]; then
  echo "::error::flake file not found: $FLAKE" >&2
  exit 1
fi

if ! printf '%s' "$NEW_HASH" | grep -qE '^sha256-[A-Za-z0-9+/]{43}=$'; then
  echo "::error::hash has unexpected shape: $NEW_HASH" >&2
  exit 1
fi

ANCHOR='^[[:space:]]*npmDepsHash[[:space:]]*=[[:space:]]*"sha256-'

MATCHES=$(grep -cE "$ANCHOR" "$FLAKE" || true)
if [ "$MATCHES" -ne 1 ]; then
  echo "::error::expected exactly 1 npmDepsHash assignment in $FLAKE, found $MATCHES" >&2
  exit 1
fi

# Portable in-place edit: write through a tempfile so this works under
# both GNU sed (`sed -i`) and BSD sed (`sed -i ''`).
TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT
sed -E "/${ANCHOR}/s|sha256-[^\"]*|${NEW_HASH}|" "$FLAKE" > "$TMP"
mv "$TMP" "$FLAKE"
