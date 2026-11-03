#!/usr/bin/env bash
# Runs the debug desktop build with the real semantic module, against your real profile.
#
#   scripts/run-dev-with-semantic.sh [--build-only]
#
# A debug build has no semantic module: the production catalog loader needs the catalog's
# *public* verifying key compiled in (release builds get it from the `desktop-release` environment
# variable SEMANTIC_CATALOG_VERIFYING_KEY_BASE64) and a signed catalog next to the binary. This
# script supplies both so the semantic settings, OCR checks and index management can be exercised:
#
#   1. reads the public verifying key with the authenticated `gh` CLI (a public value, not a secret),
#   2. copies the signed catalog + signature out of the installed /Applications/Procyon.app,
#   3. builds `fm-desktop` with PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX set, and
#   4. starts it (unless --build-only). Quit the installed app first: both would share one profile.
#
# The catalog is only trusted if its signature verifies against that key, exactly as in a release.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_resources="/Applications/Procyon.app/Contents/Resources/semantic"
repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)"

if pgrep -f "/Procyon.app/Contents/MacOS" >/dev/null; then
  echo "The installed Procyon is running; quit it first (it shares this profile)." >&2
  exit 1
fi
for file in catalog.json catalog.sig; do
  [[ -f "$app_resources/$file" ]] || { echo "missing $app_resources/$file" >&2; exit 1; }
done

key_hex="$(
  gh api "repos/$repo/environments/desktop-release/variables/SEMANTIC_CATALOG_VERIFYING_KEY_BASE64" \
    -q .value | base64 --decode | xxd -p -c 64
)"
if [[ ${#key_hex} -ne 64 ]]; then
  echo "unexpected verifying key length (${#key_hex} hex characters, want 64)" >&2
  exit 1
fi

mkdir -p "$root/target/debug/semantic"
cp -p "$app_resources/catalog.json" "$app_resources/catalog.sig" "$root/target/debug/semantic/"

export PATH="$HOME/.cargo/bin:$PATH"
(cd "$root" && PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX="$key_hex" cargo build -p fm-desktop)
(cd "$root" && VITE_RUNTIME=tauri pnpm --dir frontend build >/dev/null)

if [[ "${1:-}" == "--build-only" ]]; then
  echo "built; start it with: RUST_LOG=info $root/target/debug/fm-desktop"
  exit 0
fi
cd "$root"
(pgrep -f "vite preview" >/dev/null) || (cd frontend && pnpm exec vite preview --port 5181 --strictPort >/dev/null 2>&1 &)
sleep 2
exec env RUST_LOG=info target/debug/fm-desktop
