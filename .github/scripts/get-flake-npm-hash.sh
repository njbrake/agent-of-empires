#!/usr/bin/env bash
# Errors if `flake.nix` does not have exactly one `npmDepsHash`
# assignment, so a future second derivation cannot drift.

set -euo pipefail

FLAKE="${1:?usage: $0 <path-to-flake.nix>}"

if [ ! -f "$FLAKE" ]; then
  echo "::error::flake file not found: $FLAKE" >&2
  exit 1
fi

ANCHOR='^[[:space:]]*npmDepsHash[[:space:]]*=[[:space:]]*"sha256-'

MATCHES=$(grep -cE "$ANCHOR" "$FLAKE" || true)
if [ "$MATCHES" -ne 1 ]; then
  echo "::error::expected exactly 1 npmDepsHash assignment in $FLAKE, found $MATCHES" >&2
  exit 1
fi

grep -E "$ANCHOR" "$FLAKE" \
  | sed 's/.*npmDepsHash[[:space:]]*=[[:space:]]*"\(sha256-[^"]*\)".*/\1/'
