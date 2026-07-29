#!/usr/bin/env bash
# scripts/update-bundle.sh
#
# Rebuild or download binaries for scripts/bundle/ and update MANIFEST.json.
# Run this after a new release to keep bundled binaries up-to-date.
#
# Usage:
#   ./scripts/update-bundle.sh                    # update all bundled targets
#   ./scripts/update-bundle.sh --target x86_64-unknown-linux-musl
#   ./scripts/update-bundle.sh --version v0.2.0  # pin a version
#
# Prerequisites:
#   - curl, sha256sum, jq or python3 (for JSON manipulation)
#   - For local builds: Rust toolchain with musl target
#   - For downloads: internet access to GitHub Releases

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE_DIR="${SCRIPT_DIR}/bundle"
REPO="topic-hash/codespacectl"
VERSION=""
TARGET=""

# --- arg parsing ------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)  VERSION="$2"; shift 2 ;;
    --target)   TARGET="$2"; shift 2 ;;
    --repo)     REPO="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,8p' "$0"
      exit 0 ;;
    *)
      echo "update-bundle.sh: unknown flag: $1" >&2
      exit 1 ;;
  esac
done

# --- resolve latest version if not pinned -----------------------------------
if [[ -z "$VERSION" ]]; then
  VERSION="$(curl -fsIL -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest" 2>/dev/null \
    | grep -oE 'tag/[^/]+$' | sed 's|^tag/||' || true)"
  [[ -z "$VERSION" ]] && echo "ERROR: could not resolve latest version" >&2 && exit 1
fi

echo "==> Updating bundle for ${VERSION}"

# --- define bundled targets ------------------------------------------------
# These are the platforms we bundle for. Extend this list as needed.
if [[ -z "$TARGET" ]]; then
  TARGETS=(
    "x86_64-unknown-linux-musl"
  )
else
  TARGETS=("$TARGET")
fi

mkdir -p "$BUNDLE_DIR"

# --- JSON helper (python3 always available) ----------------------------------
update_manifest_json() {
  local version="$1"
  local target="$2"
  local filename="$3"
  local sha="$4"

  python3 << PYEOF
import json, os

manifest_path = "${BUNDLE_DIR}/MANIFEST.json"
entry = {
    "target": "${target}",
    "file": "${filename}",
    "sha256": "${sha}"
}

if os.path.exists(manifest_path):
    with open(manifest_path) as f:
        manifest = json.load(f)
else:
    manifest = {
        "\$schema": "https://github.com/${REPO}/schemas/bundle-manifest-v1",
        "version": "${version}",
        "binaries": []
    }

manifest["version"] = "${version}"

# Update or add entry
found = False
for i, b in enumerate(manifest["binaries"]):
    if b["target"] == "${target}":
        manifest["binaries"][i] = entry
        found = True
        break
if not found:
    manifest["binaries"].append(entry)

with open(manifest_path, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")

print(f"  updated MANIFEST.json: ${target} -> ${sha}")
PYEOF
}

# --- download + verify for each target -------------------------------------
for tgt in "${TARGETS[@]}"; do
  case "$tgt" in
    *windows*) ext="zip";   bin="codespacectl.exe" ;;
    *)         ext="tar.gz"; bin="codespacectl" ;;
  esac

  asset="codespacectl-${VERSION}-${tgt}.${ext}"
  url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
  dest="${BUNDLE_DIR}/codespacectl-${tgt}"

  echo "==> Downloading ${asset}..."

  # Download archive
  tmp_archive="$(mktemp)"
  if ! curl -fsSL "$url" -o "$tmp_archive"; then
    echo "ERROR: failed to download ${url}" >&2
    rm -f "$tmp_archive"
    exit 1
  fi

  # Verify against published SHA256SUMS
  sha_url="https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS.txt"
  tmp_sums="$(mktemp)"
  curl -fsSL "$sha_url" -o "$tmp_sums"
  expected_sha="$(grep " ${asset}\$" "$tmp_sums" | awk '{print $1}')"
  rm -f "$tmp_sums"

  actual_sha="$(sha256sum "$tmp_archive" | awk '{print $1}')"
  if [[ "$expected_sha" != "$actual_sha" ]]; then
    echo "ERROR: SHA-256 mismatch for ${asset}" >&2
    echo "  expected: ${expected_sha}" >&2
    echo "  actual:   ${actual_sha}" >&2
    rm -f "$tmp_archive"
    exit 1
  fi
  echo "  SHA-256 verified: ${actual_sha}"

  # Extract binary
  tmp_extract="$(mktemp -d)"
  if [[ "$ext" == "tar.gz" ]]; then
    tar xzf "$tmp_archive" -C "$tmp_extract"
  else
    unzip -q "$tmp_archive" -d "$tmp_extract"
  fi
  rm -f "$tmp_archive"

  extracted="$(find "$tmp_extract" -type f -name "$bin" | head -1)"
  if [[ -z "$extracted" ]]; then
    echo "ERROR: could not find ${bin} in archive" >&2
    rm -rf "$tmp_extract"
    exit 1
  fi

  # Install to bundle dir
  cp "$extracted" "$dest"
  chmod 0755 "$dest"
  rm -rf "$tmp_extract"

  # Get SHA of the extracted binary (not the archive)
  bin_sha="$(sha256sum "$dest" | awk '{print $1}')"

  # Update manifest
  update_manifest_json "$VERSION" "$tgt" "codespacectl-${tgt}" "$bin_sha"

  echo "  bundled: ${dest} (${bin_sha})"
done

echo ""
echo "==> Bundle updated for ${VERSION}"
echo "    Targets: ${TARGETS[*]}"
echo "    Manifest: ${BUNDLE_DIR}/MANIFEST.json"
ls -lh "${BUNDLE_DIR}/codespacectl-"*
