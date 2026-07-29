#!/usr/bin/env bash
# scripts/bootstrap.sh
#
# Self-installer for codespacectl. Mirrors the lookup pattern used by
# src/github/gh_downloader.rs for the gh CLI dependency, but applied to
# codespacectl itself. No sudo. No system packages. Single static binary.
#
# Lookup order (first match wins):
#   0. Bundled binary in scripts/bundle/ matching current platform (offline)
#   1. $CODESPACECTL_BIN env var (explicit override)
#   2. Existing install at $INSTALL_DIR/codespacectl (skip unless --upgrade)
#      - Platform mismatch: auto-delete stale binary and re-download
#   3. Cache at $CACHE_DIR/codespacectl
#   4. Download from GitHub Releases + verify SHA-256 + install
#
# Defaults (XDG-friendly, no sudo):
#   INSTALL_DIR = $HOME/.local/bin
#   CACHE_DIR   = $HOME/.cache/codespacectl/bin
#
# Usage:
#   curl -fsSL https://github.com/topic-hash/codespacectl/raw/main/scripts/bootstrap.sh | bash
#   curl -fsSL ... | bash -s -- --version v0.1.0      # pin a version
#   curl -fsSL ... | bash -s -- --upgrade             # force re-download
#   curl -fsSL ... | bash -s -- --install-dir /opt/bin
#
# Exit codes:
#   0  success (binary is now usable on PATH or at printed path)
#   1  usage error
#   2  unsupported platform
#   3  download failed
#   4  SHA-256 verification failed
#   5  extraction failed
#   6  install (mv/chmod) failed

set -euo pipefail

# --- defaults ---------------------------------------------------------------
REPO="topic-hash/codespacectl"
INSTALL_DIR="${HOME}/.local/bin"
CACHE_DIR="${HOME}/.cache/codespacectl/bin"
VERSION=""        # empty -> query /releases/latest
UPGRADE=0
VERBOSE=0

# --- arg parsing ------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)      VERSION="$2"; shift 2 ;;
    --upgrade)      UPGRADE=1; shift ;;
    --install-dir)  INSTALL_DIR="$2"; shift 2 ;;
    --cache-dir)    CACHE_DIR="$2"; shift 2 ;;
    --repo)         REPO="$2"; shift 2 ;;
    -v|--verbose)   VERBOSE=1; shift ;;
    -h|--help)
      sed -n '2,28p' "$0"
      exit 0 ;;
    *)
      echo "bootstrap.sh: unknown flag: $1" >&2
      exit 1 ;;
  esac
done

log()   { [[ $VERBOSE -eq 1 ]] && echo "[bootstrap] $*" >&2 || true; }
die()   { echo "[bootstrap] ERROR: $*" >&2; exit "${2:-1}"; }
note()  { echo "[bootstrap] $*"; }

# --- platform detection (mirrors gh_downloader.rs::platform_asset_name) -----
detect_target() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os/$arch" in
    linux/x86_64)             echo "x86_64-unknown-linux-musl" ;;
    linux/aarch64|linux/arm64) echo "aarch64-unknown-linux-musl" ;;
    darwin/x86_64)            echo "x86_64-apple-darwin" ;;
    darwin/arm64|darwin/aarch64) echo "aarch64-apple-darwin" ;;
    mingw32*/x86_64|msys*/x86_64|cygwin*/x86_64) echo "x86_64-pc-windows-gnu" ;;
    *) die "unsupported platform: os=$os arch=$arch" 2 ;;
  esac
}

# archive extension per target (matches release.yml matrix)
archive_ext() {
  case "$1" in
    *windows*) echo "zip" ;;
    *)         echo "tar.gz" ;;
  esac
}

# binary name inside the archive (matches release.yml packaging step)
binary_name() {
  case "$1" in
    *windows*) echo "codespacectl.exe" ;;
    *)         echo "codespacectl" ;;
  esac
}

# Detect platform of an existing ELF/Mach-O binary.
# Returns the target triple if detectable, empty string otherwise.
detect_binary_platform() {
  local bin="$1"
  [[ ! -f "$bin" ]] && return 0

  # Try ELF (Linux)
  if file "$bin" 2>/dev/null | grep -qi "ELF"; then
    local arch machine
    arch="$(file "$bin" 2>/dev/null | grep -oi 'ELF [0-9]+-bit' || true)"
    machine="$(file "$bin" 2>/dev/null | grep -oi 'X86-64\|ARM aarch64\|80386' || true)"

    if echo "$machine" | grep -qi "X86-64"; then
      # Check for musl vs gnu
      if ldd "$bin" 2>/dev/null | grep -qi "musl\|not a dynamic executable\|statically linked"; then
        echo "x86_64-unknown-linux-musl"
      else
        echo "x86_64-unknown-linux-gnu"
      fi
    elif echo "$machine" | grep -qi "ARM aarch64"; then
      echo "aarch64-unknown-linux-musl"
    fi
    return 0
  fi

  # Try Mach-O (macOS)
  if file "$bin" 2>/dev/null | grep -qi "Mach-O"; then
    if file "$bin" 2>/dev/null | grep -qi "x86_64"; then
      echo "x86_64-apple-darwin"
    elif file "$bin" 2>/dev/null | grep -qi "arm64"; then
      echo "aarch64-apple-darwin"
    fi
    return 0
  fi

  # Try PE (Windows)
  if file "$bin" 2>/dev/null | grep -qi "PE32+\|PE32"; then
    echo "x86_64-pc-windows-gnu"
    return 0
  fi
}

# --- main -------------------------------------------------------------------
TARGET="$(detect_target)"
EXT="$(archive_ext "$TARGET")"
BIN_NAME="$(binary_name "$TARGET")"
ASSET="codespacectl-${VERSION}-${TARGET}.${EXT}"

# =============================================================================
# EARLY EXIT TIERS (0-2): no network needed
# These run BEFORE any version resolution to avoid unnecessary GitHub calls.
# =============================================================================

# --- tier 0: bundled binary (repo ships pre-compiled binaries for common
#     targets — zero network, zero download, ideal for sandboxes / CI) ------
# Determine bundle dir: look for scripts/bundle/ relative to this script,
# then fall back to the repo root if bootstrapped from a clone.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE_DIR="${SCRIPT_DIR}/bundle"
if [[ ! -d "$BUNDLE_DIR" ]]; then
  BUNDLE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)/scripts/bundle"
fi

if [[ -d "$BUNDLE_DIR" && $UPGRADE -eq 0 ]]; then
  BUNDLED_BIN="${BUNDLE_DIR}/codespacectl-${TARGET}"
  if [[ -x "$BUNDLED_BIN" ]]; then
    log "found bundled binary for ${TARGET} at ${BUNDLED_BIN}"
    # Verify against MANIFEST.json if present
    if [[ -f "${BUNDLE_DIR}/MANIFEST.json" ]]; then
      BUNDLED_SHA="$(sha256sum "$BUNDLED_BIN" | awk '{print $1}')"
      EXPECTED_BUNDLED_SHA="$(python3 -c "
import json, sys
with open('${BUNDLE_DIR}/MANIFEST.json') as f:
    manifest = json.load(f)
for b in manifest.get('binaries', []):
    if b.get('target') == '${TARGET}':
        print(b.get('sha256', ''))
        sys.exit(0)
" 2>/dev/null || true)"
      if [[ -n "$EXPECTED_BUNDLED_SHA" && "$BUNDLED_SHA" != "$EXPECTED_BUNDLED_SHA" ]]; then
        log "bundled binary sha256 mismatch (expected ${EXPECTED_BUNDLED_SHA}, got ${BUNDLED_SHA}), skipping"
      else
        log "bundled binary sha256 verified"
        mkdir -p "$INSTALL_DIR"
        cp "$BUNDLED_BIN" "${INSTALL_DIR}/${BIN_NAME}"
        chmod 0755 "${INSTALL_DIR}/${BIN_NAME}"
        note "installed ${INSTALL_DIR}/${BIN_NAME} (from bundled binary)"
        echo "${INSTALL_DIR}/${BIN_NAME}"
        exit 0
      fi
    else
      # No manifest — trust the binary (local dev / trusted clone)
      mkdir -p "$INSTALL_DIR"
      cp "$BUNDLED_BIN" "${INSTALL_DIR}/${BIN_NAME}"
      chmod 0755 "${INSTALL_DIR}/${BIN_NAME}"
      note "installed ${INSTALL_DIR}/${BIN_NAME} (from bundled binary, no manifest)"
      echo "${INSTALL_DIR}/${BIN_NAME}"
      exit 0
    fi
  fi
fi

# --- tier 1: explicit env override -----------------------------------------
if [[ -n "${CODESPACECTL_BIN:-}" && -x "${CODESPACECTL_BIN}" && $UPGRADE -eq 0 ]]; then
  # Check platform mismatch for env override
  BIN_PLATFORM="$(detect_binary_platform "${CODESPACECTL_BIN}")"
  if [[ -n "$BIN_PLATFORM" && "$BIN_PLATFORM" != "$TARGET" ]]; then
    log "CODESPACECTL_BIN binary is for ${BIN_PLATFORM}, current system is ${TARGET} — skipping"
  else
    note "found \$CODESPACECTL_BIN at ${CODESPACECTL_BIN}, nothing to do"
    echo "${CODESPACECTL_BIN}"
    exit 0
  fi
fi

# --- tier 2: existing install at INSTALL_DIR -------------------------------
EXISTING="${INSTALL_DIR}/${BIN_NAME}"
if [[ -x "$EXISTING" && $UPGRADE -eq 0 ]]; then
  # Platform mismatch detection: if the installed binary is for a different
  # architecture, remove it and fall through to download the correct one.
  BIN_PLATFORM="$(detect_binary_platform "$EXISTING")"
  if [[ -n "$BIN_PLATFORM" && "$BIN_PLATFORM" != "$TARGET" ]]; then
    note "installed binary is for ${BIN_PLATFORM}, current system is ${TARGET} — removing stale binary"
    rm -f "$EXISTING"
    # Also remove cache if present
    CACHE_BIN_OLD="${CACHE_DIR}/${BIN_NAME}"
    if [[ -f "$CACHE_BIN_OLD" ]]; then
      log "also removing stale cache entry at ${CACHE_BIN_OLD}"
      rm -f "$CACHE_BIN_OLD" "${CACHE_BIN_OLD}.sha256"
    fi
  else
    # Same platform — check version (requires knowing the latest version)
    # For now, skip version check if version not yet resolved.
    # We'll come back to this after version resolution if needed.
    if [[ -n "$VERSION" ]]; then
      INSTALLED_VER="$("$EXISTING" --version 2>/dev/null | awk '{print $2}' || true)"
      if [[ "$INSTALLED_VER" == "${VERSION#v}" ]]; then
        note "already installed at ${EXISTING} (${VERSION}), nothing to do"
        echo "$EXISTING"
        exit 0
      fi
      log "existing install is ${INSTALLED_VER:-unknown}, upgrading to ${VERSION}"
    fi
    # Version not resolved yet but binary is correct platform — save this
    # for post-resolution check below
    EXISTING_OK_PLATFORM=1
  fi
fi

# =============================================================================
# VERSION RESOLUTION (network — only reached if early exits didn't match)
# =============================================================================
if [[ -z "$VERSION" ]]; then
  log "resolving latest release tag via redirect"
  VERSION="$(curl -fsIL -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest" 2>/dev/null \
    | grep -oE 'tag/[^/]+$' | sed 's|^tag/||' || true)"

  if [[ -z "$VERSION" ]]; then
    log "redirect failed, falling back to api.github.com"
    VERSION="$(curl -fsSL \
      -H "Accept: application/vnd.github+json" \
      "https://api.github.com/repos/${REPO}/releases/latest" \
      | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])' 2>/dev/null \
      || true)"
  fi
  [[ -z "$VERSION" ]] && die "could not resolve latest release tag" 3
  ASSET="codespacectl-${VERSION}-${TARGET}.${EXT}"
fi
note "installing codespacectl ${VERSION} for ${TARGET}"

# --- tier 2 (retry): version-gated check for existing install ----------------
# If we had a correct-platform binary but couldn't check version earlier,
# check it now that we have the version.
if [[ "${EXISTING_OK_PLATFORM:-0}" -eq 1 && -x "$EXISTING" && $UPGRADE -eq 0 ]]; then
  INSTALLED_VER="$("$EXISTING" --version 2>/dev/null | awk '{print $2}' || true)"
  if [[ "$INSTALLED_VER" == "${VERSION#v}" ]]; then
    note "already installed at ${EXISTING} (${VERSION}), nothing to do"
    echo "$EXISTING"
    exit 0
  fi
  log "existing install is ${INSTALLED_VER:-unknown}, upgrading to ${VERSION}"
fi

# --- tier 3: cache hit ------------------------------------------------------
CACHE_BIN="${CACHE_DIR}/${BIN_NAME}"
CACHE_SHA="${CACHE_DIR}/${BIN_NAME}.sha256"
if [[ -x "$CACHE_BIN" && $UPGRADE -eq 0 ]]; then
  # Verify cache binary matches current platform
  CACHE_PLATFORM="$(detect_binary_platform "$CACHE_BIN")"
  if [[ -n "$CACHE_PLATFORM" && "$CACHE_PLATFORM" != "$TARGET" ]]; then
    log "cache binary is for ${CACHE_PLATFORM}, current system is ${TARGET} — removing stale cache"
    rm -f "$CACHE_BIN" "${CACHE_SHA}"
  else
    CACHED_VER="$("$CACHE_BIN" --version 2>/dev/null | awk '{print $2}' || true)"
    if [[ "$CACHED_VER" == "${VERSION#v}" ]]; then
      log "cache hit at ${CACHE_BIN}"
      mkdir -p "$INSTALL_DIR"
      cp "$CACHE_BIN" "$EXISTING"
      chmod 0755 "$EXISTING"
      note "installed ${EXISTING} (from cache)"
      echo "$EXISTING"
      exit 0
    fi
  fi
fi

# --- tier 4: download + verify + install -----------------------------------
DOWNLOAD_BASE="https://github.com/${REPO}/releases/download"
mkdir -p "$CACHE_DIR"

# 4a. download SHA256SUMS.txt
SHA_URL="${DOWNLOAD_BASE}/${VERSION}/SHA256SUMS.txt"
log "downloading ${SHA_URL}"
if ! curl -fsSL "$SHA_URL" -o "${CACHE_DIR}/SHA256SUMS.txt"; then
  die "failed to download SHA256SUMS.txt from ${SHA_URL}" 3
fi

# 4b. extract expected SHA for our asset
EXPECTED_SHA="$(grep " ${ASSET}\$" "${CACHE_DIR}/SHA256SUMS.txt" | awk '{print $1}' || true)"
[[ -z "$EXPECTED_SHA" ]] && die "asset ${ASSET} not found in SHA256SUMS.txt" 4

# 4c. download the asset
ASSET_URL="${DOWNLOAD_BASE}/${VERSION}/${ASSET}"
log "downloading ${ASSET_URL}"
ASSET_PATH="${CACHE_DIR}/${ASSET}"
if ! curl -fsSL "$ASSET_URL" -o "$ASSET_PATH"; then
  die "failed to download ${ASSET_URL}" 3
fi

# 4d. verify SHA-256
ACTUAL_SHA="$(sha256sum "$ASSET_PATH" | awk '{print $1}')"
if [[ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]]; then
  echo "[bootstrap] expected sha: $EXPECTED_SHA" >&2
  echo "[bootstrap] actual sha:   $ACTUAL_SHA" >&2
  rm -f "$ASSET_PATH"
  die "SHA-256 verification failed for ${ASSET}" 4
fi
note "sha256 verified: ${ACTUAL_SHA}"

# 4e. extract the binary from the archive
TMP_EXTRACT="$(mktemp -d)"
trap 'rm -rf "$TMP_EXTRACT"' EXIT

if [[ "$EXT" == "tar.gz" ]]; then
  if ! tar xzf "$ASSET_PATH" -C "$TMP_EXTRACT"; then
    die "tar extraction failed" 5
  fi
elif [[ "$EXT" == "zip" ]]; then
  if ! unzip -q "$ASSET_PATH" -d "$TMP_EXTRACT"; then
    die "zip extraction failed" 5
  fi
fi

EXTRACTED_BIN="$(find "$TMP_EXTRACT" -type f -name "$BIN_NAME" | head -1)"
[[ -z "$EXTRACTED_BIN" ]] && die "could not find ${BIN_NAME} in archive" 5

# 4f. install to cache, then to INSTALL_DIR
mkdir -p "$INSTALL_DIR"
cp "$EXTRACTED_BIN" "$CACHE_BIN"
chmod 0755 "$CACHE_BIN"
cp "$EXTRACTED_BIN" "$EXISTING"
chmod 0755 "$EXISTING"

# persist the verified sha next to the cache binary (mirrors gh_downloader.rs)
echo "${ACTUAL_SHA}  ${BIN_NAME}" > "$CACHE_SHA"

# 4g. PATH hint (only if INSTALL_DIR is not on PATH)
case ":${PATH:-}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    note "add ${INSTALL_DIR} to your PATH:"
    note "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc  # or ~/.zshrc"
    ;;
esac

note "installed ${EXISTING}"
"$EXISTING" --version
echo "$EXISTING"
