#!/usr/bin/env bash
# scripts/bootstrap.sh
#
# Self-installer for codespacectl. Mirrors the lookup pattern used by
# src/github/gh_downloader.rs for the gh CLI dependency, but applied to
# codespacectl itself. No sudo. No system packages. Single static binary.
#
# Lookup order (first match wins):
#   1. $CODESPACECTL_BIN env var (explicit override)
#   2. Existing install at $INSTALL_DIR/codespacectl (skip unless --upgrade)
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
VERSION=""        # empty → query /releases/latest
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

# --- main -------------------------------------------------------------------
TARGET="$(detect_target)"
EXT="$(archive_ext "$TARGET")"
BIN_NAME="$(binary_name "$TARGET")"
ASSET="codespacectl-${VERSION}-${TARGET}.${EXT}"

# Resolve version if not pinned.
# Prefer the HTML redirect endpoint (no API call, no rate limit, 60→inf).
# Fall back to the REST API if the redirect fails (e.g. behind a proxy that
# strips redirects).
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

# --- tier 1: explicit env override -----------------------------------------
if [[ -n "${CODESPACECTL_BIN:-}" && -x "${CODESPACECTL_BIN}" && $UPGRADE -eq 0 ]]; then
  note "found \$CODESPACECTL_BIN at ${CODESPACECTL_BIN}, nothing to do"
  echo "${CODESPACECTL_BIN}"
  exit 0
fi

# --- tier 2: existing install at INSTALL_DIR -------------------------------
EXISTING="${INSTALL_DIR}/${BIN_NAME}"
if [[ -x "$EXISTING" && $UPGRADE -eq 0 ]]; then
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
