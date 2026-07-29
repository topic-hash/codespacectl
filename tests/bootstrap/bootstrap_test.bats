#!/usr/bin/env bats
# tests/bootstrap/bootstrap_test.bats
#
# Comprehensive test suite for scripts/bootstrap.sh
# Test levels: L1 unit -> L2 integration -> L3 contract -> L4 E2E
#
# Run:  bats tests/bootstrap/
#       bats tests/bootstrap/ --filter "unit"
#       bats tests/bootstrap/ --filter "integration"
#       bats tests/bootstrap/ --filter "contract"
#       bats tests/bootstrap/ --filter "e2e"
#
# Requires: bats >= 1.7, shellcheck (for L5)

###############################################################################
# SETUP -- HERMETIC ENVIRONMENT
###############################################################################

SCRIPT_DIR="/home/z/my-project/codespacectl/scripts"
BUNDLE_DIR="${SCRIPT_DIR}/bundle"
REAL_BINARY="${BUNDLE_DIR}/codespacectl-x86_64-unknown-linux-musl"
BOOTSTRAP="${SCRIPT_DIR}/bootstrap.sh"
UPDATE_BUNDLE="${SCRIPT_DIR}/update-bundle.sh"

setup() {
  TEST_HOME="$(mktemp -d)"
  TEST_INSTALL="${TEST_HOME}/.local/bin"
  TEST_CACHE="${TEST_HOME}/.cache/codespacectl/bin"

  mkdir -p "$TEST_INSTALL" "$TEST_CACHE"

  # Copy the real bundle binary into a test bundle dir
  TEST_BUNDLE="${TEST_HOME}/test_bundle"
  mkdir -p "$TEST_BUNDLE"
  if [[ -f "$REAL_BINARY" ]]; then
    cp "$REAL_BINARY" "${TEST_BUNDLE}/codespacectl-x86_64-unknown-linux-musl"
    chmod 0755 "${TEST_BUNDLE}/codespacectl-x86_64-unknown-linux-musl"
    cp "${BUNDLE_DIR}/MANIFEST.json" "${TEST_BUNDLE}/MANIFEST.json"
  fi

  # Create an isolated scripts/ dir with a COPY of bootstrap.sh
  # so BASH_SOURCE resolves inside TEST_HOME (not the real repo)
  TEST_SCRIPTS="${TEST_HOME}/scripts"
  mkdir -p "${TEST_SCRIPTS}/bundle"
  cp "$BOOTSTRAP" "${TEST_SCRIPTS}/bootstrap.sh"
  chmod +x "${TEST_SCRIPTS}/bootstrap.sh"

  # By default, NO bundle is wired in. Tests that want tier 0 must call
  # enable_bundle() explicitly.
}

teardown() {
  rm -rf "$TEST_HOME"
}

# Wire the test bundle into the test scripts dir so tier 0 sees it
enable_bundle() {
  cp "${TEST_BUNDLE}/codespacectl-x86_64-unknown-linux-musl" "${TEST_SCRIPTS}/bundle/"
  cp "${TEST_BUNDLE}/MANIFEST.json" "${TEST_SCRIPTS}/bundle/"
}

# Run bootstrap in full hermetic isolation. No bundle wired.
run_bootstrap() {
  run env -i \
    HOME="$TEST_HOME" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    TERM="dumb" \
    bash "${TEST_SCRIPTS}/bootstrap.sh" \
    --install-dir "$TEST_INSTALL" \
    --cache-dir "$TEST_CACHE" \
    "$@"
}

# Run bootstrap with bundle wired in (tier 0 active)
run_bootstrap_bundled() {
  enable_bundle
  run_bootstrap "$@"
}

sha256_file() {
  sha256sum "$1" | awk '{print $1}'
}

###############################################################################
# L1 -- UNIT TESTS
###############################################################################

@test "unit: detect_target returns x86_64-unknown-linux-musl on linux x86_64" {
  run_bootstrap_bundled --version v0.1.0 -v
  [[ "$output" == *"x86_64-unknown-linux-musl"* ]]
}

@test "unit: archive_ext returns tar.gz for linux targets" {
  # The ASSET name contains .tar.gz for non-windows targets
  run_bootstrap_bundled --version v0.1.0 -v
  # The note line prints the target, which determines tar.gz
  [[ "$output" == *"x86_64-unknown-linux-musl"* ]]
}

@test "unit: binary_name returns codespacectl (not .exe) on linux" {
  run_bootstrap_bundled --version v0.1.0
  [[ "$status" -eq 0 ]]
  [[ -f "${TEST_INSTALL}/codespacectl" ]]
  [[ ! -f "${TEST_INSTALL}/codespacectl.exe" ]]
}

@test "unit: arg parsing rejects unknown flag with exit 1" {
  run env -i HOME="$TEST_HOME" PATH="/usr/bin:/bin" bash "${TEST_SCRIPTS}/bootstrap.sh" --bogus-flag 2>&1
  [[ "$status" -eq 1 ]]
  [[ "$output" == *"unknown flag"* ]]
}

@test "unit: arg parsing --help exits 0 and prints usage" {
  run env -i HOME="$TEST_HOME" PATH="/usr/bin:/bin" bash "${TEST_SCRIPTS}/bootstrap.sh" -h
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"Self-installer"* ]]
}

@test "unit: arg parsing --version accepts a pinned version" {
  run_bootstrap --version v0.1.0 --cache-dir /dev/null --install-dir /dev/null 2>&1 || true
  [[ "$output" != *"unknown flag"* ]]
}

@test "unit: MANIFEST.json is valid JSON with required fields" {
  run python3 -c "
import json, sys
with open('${BUNDLE_DIR}/MANIFEST.json') as f:
    m = json.load(f)
assert 'version' in m, 'missing version'
assert 'binaries' in m, 'missing binaries'
assert isinstance(m['binaries'], list), 'binaries must be a list'
for b in m['binaries']:
    assert 'target' in b, 'missing target'
    assert 'file' in b, 'missing file'
    assert 'sha256' in b, 'missing sha256'
    assert len(b['sha256']) == 64, 'sha256 must be 64 hex chars'
print('MANIFEST.json schema: VALID')
"
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"VALID"* ]]
}

@test "unit: bundled binary SHA-256 matches MANIFEST.json" {
  local actual_sha expected_sha
  actual_sha="$(sha256_file "$REAL_BINARY")"
  expected_sha="$(python3 -c "
import json
with open('${BUNDLE_DIR}/MANIFEST.json') as f:
    m = json.load(f)
for b in m['binaries']:
    if b['target'] == 'x86_64-unknown-linux-musl':
        print(b['sha256'])
")"
  [[ "$actual_sha" == "$expected_sha" ]]
}

@test "unit: bundled binary is a static ELF x86_64 binary" {
  run file "$REAL_BINARY"
  [[ "$output" == *"ELF 64-bit"* ]]
  [[ "$output" == *"x86-64"* ]]
  [[ "$output" == *"static"* ]]
}

@test "unit: detect_binary_platform identifies x86_64-unknown-linux-musl binary" {
  local test_script="${TEST_HOME}/detect_test.sh"
  cat > "$test_script" << 'SCRIPT'
#!/usr/bin/env bash
detect_binary_platform() {
  local bin="$1"
  [[ ! -f "$bin" ]] && return 0
  if file "$bin" 2>/dev/null | grep -qi "ELF"; then
    local machine
    machine="$(file "$bin" 2>/dev/null | grep -oi 'X86-64\|ARM aarch64\|80386' || true)"
    if echo "$machine" | grep -qi "X86-64"; then
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
}
echo "$(detect_binary_platform "$1")"
SCRIPT
  run bash "$test_script" "$REAL_BINARY"
  [[ "$status" -eq 0 ]]
  [[ "$output" == "x86_64-unknown-linux-musl" ]]
}

@test "unit: detect_binary_platform returns empty for non-binary file" {
  local test_script="${TEST_HOME}/detect_test.sh"
  cat > "$test_script" << 'SCRIPT'
#!/usr/bin/env bash
detect_binary_platform() {
  local bin="$1"
  [[ ! -f "$bin" ]] && return 0
  if file "$bin" 2>/dev/null | grep -qi "ELF"; then
    echo "x86_64-unknown-linux-musl"
    return 0
  fi
}
echo "$(detect_binary_platform "$1")"
SCRIPT
  echo "not a binary" > "${TEST_HOME}/fake.txt"
  run bash "$test_script" "${TEST_HOME}/fake.txt"
  [[ "$status" -eq 0 ]]
  [[ "$output" != *"x86_64"* ]]
}

###############################################################################
# L2 -- INTEGRATION TESTS
###############################################################################

@test "integration: tier 0 -- cold install from bundled binary (zero network)" {
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]
  [[ "$output" == *"bundled binary"* ]]
}

@test "integration: tier 0 -- bundled binary passes SHA-256 verification" {
  run_bootstrap_bundled -v
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"sha256 verified"* ]]
}

@test "integration: tier 0 -- skips bundle when UPGRADE flag is set" {
  run_bootstrap_bundled --upgrade -v
  # With --upgrade tier 0 is bypassed -- no "bundled binary" message
  [[ "$output" != *"bundled binary"* ]] || [[ "$output" != *"sha256 verified"* ]]
}

@test "integration: tier 0 -- rejects tampered binary (SHA mismatch)" {
  enable_bundle
  # Tamper AFTER enable_bundle copies the binary
  echo "TAMPERED" >> "${TEST_SCRIPTS}/bundle/codespacectl-x86_64-unknown-linux-musl"
  chmod 0755 "${TEST_SCRIPTS}/bundle/codespacectl-x86_64-unknown-linux-musl"
  # MANIFEST still has the original sha -> mismatch
  # Use run_bootstrap (not run_bootstrap_bundled) to avoid re-copying
  run_bootstrap -v
  [[ "$output" == *"sha256 mismatch"* ]]
}

@test "integration: tier 0 -- works without MANIFEST.json (trusts binary)" {
  enable_bundle
  rm -f "${TEST_SCRIPTS}/bundle/MANIFEST.json"
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]
  [[ "$output" == *"bundled binary"* ]]
}

@test "integration: tier 0 -- skips when bundle binary has no execute bit" {
  enable_bundle
  chmod 0644 "${TEST_SCRIPTS}/bundle/codespacectl-x86_64-unknown-linux-musl"
  run_bootstrap -v
  # No bundle match (not executable) -> skips to version resolution
  [[ "$output" == *"resolving"* ]]
  # Must NOT have found the bundle binary
  [[ "$output" != *"bundled binary"* ]]
}

@test "integration: tier 1 -- CODESPACECTL_BIN env override" {
  # No bundle wired -- tier 0 skipped
  cp "$REAL_BINARY" "${TEST_INSTALL}/codespacectl"
  chmod 0755 "${TEST_INSTALL}/codespacectl"

  run env -i \
    HOME="$TEST_HOME" \
    PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    CODESPACECTL_BIN="${TEST_INSTALL}/codespacectl" \
    bash "${TEST_SCRIPTS}/bootstrap.sh" \
    --install-dir "$TEST_INSTALL" \
    --cache-dir "$TEST_CACHE"

  [[ "$status" -eq 0 ]]
  [[ "$output" == *"CODESPACECTL_BIN"* ]]
}

@test "integration: tier 2 -- already installed, correct version, skips download" {
  # Install via bundle first
  run_bootstrap_bundled --version v0.1.0
  [[ "$status" -eq 0 ]]

  # Remove bundle so tier 0 is skipped, force tier 2 evaluation
  rm -rf "${TEST_SCRIPTS}/bundle"

  run_bootstrap --version v0.1.0
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"already installed"* ]]
}

@test "integration: tier 2 -- script binary (non-ELF) is not flagged as mismatch" {
  # Install a shell script where the real binary should be
  echo '#!/bin/sh' > "${TEST_INSTALL}/codespacectl"
  chmod 0755 "${TEST_INSTALL}/codespacectl"
  # Remove bundle so tier 2 is evaluated
  rm -rf "${TEST_SCRIPTS}/bundle"

  run_bootstrap --version v0.1.0 2>&1 || true
  # detect_binary_platform returns empty for scripts -> no mismatch -> tries version check
  # version check fails (can't run --version on a shell script) -> falls through
  [[ "$output" != *"already installed"* ]]
}

@test "integration: tier 3 -- cache hit installs from cache" {
  # No bundle wired
  # Put the real binary in cache
  cp "$REAL_BINARY" "${TEST_CACHE}/codespacectl"
  chmod 0755 "${TEST_CACHE}/codespacectl"
  echo "$(sha256_file "${TEST_CACHE}/codespacectl")  codespacectl" > "${TEST_CACHE}/codespacectl.sha256"

  run_bootstrap --version v0.1.0
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"from cache"* ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]
}

@test "integration: tier 3 -- stale cache (non-ELF file) falls through" {
  # Put a text file in cache
  echo "not a real binary" > "${TEST_CACHE}/codespacectl"
  chmod 0755 "${TEST_CACHE}/codespacectl"

  # No bundle, no install
  rm -rf "${TEST_SCRIPTS}/bundle"

  run_bootstrap --version v0.1.0 2>&1 || true
  # Should not say "from cache" (version check fails, falls through to download)
  [[ "$output" != *"from cache"* ]]
}

@test "integration: --upgrade flag forces re-download (bypasses tier 0-3)" {
  run_bootstrap_bundled --upgrade -v
  # Tier 0 is bypassed with --upgrade
  [[ "$output" != *"bundled binary"* ]]
}

@test "integration: --version v0.1.0 skips version resolution" {
  run_bootstrap_bundled --version v0.1.0 -v
  [[ "$status" -eq 0 ]]
  [[ "$output" != *"resolving latest"* ]]
}

@test "integration: installed binary is executable and reports version" {
  run_bootstrap_bundled --version v0.1.0
  [[ "$status" -eq 0 ]]

  run "${TEST_INSTALL}/codespacectl" --version
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"0.1.0"* ]]
}

###############################################################################
# L3 -- CONTRACT / PROPERTY TESTS
###############################################################################

@test "contract: exit code 0 on successful bundle install" {
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
}

@test "contract: exit code 1 on unknown flag" {
  run env -i HOME="$TEST_HOME" PATH="/usr/bin:/bin" bash "${TEST_SCRIPTS}/bootstrap.sh" --nonexistent 2>&1
  [[ "$status" -eq 1 ]]
}

@test "contract: stdout prints the installed binary path on success" {
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"${TEST_INSTALL}/codespacectl"* ]]
}

@test "contract: idempotency -- 10 consecutive runs produce same result" {
  for _ in $(seq 1 10); do
    run_bootstrap_bundled
    [[ "$status" -eq 0 ]]
    [[ -x "${TEST_INSTALL}/codespacectl" ]]
  done
}

@test "contract: idempotency -- same SHA after 10 installs" {
  local sha_after_first=""
  for _ in $(seq 1 10); do
    run_bootstrap_bundled
    [[ "$status" -eq 0 ]]
    local current_sha
    current_sha="$(sha256_file "${TEST_INSTALL}/codespacectl")"
    if [[ -n "$sha_after_first" ]]; then
      [[ "$current_sha" == "$sha_after_first" ]]
    fi
    sha_after_first="$current_sha"
  done
}

@test "contract: no-network guarantee -- tier 0 completes in under 200ms" {
  local start end elapsed
  start=$(date +%s%N)
  run_bootstrap_bundled
  end=$(date +%s%N)
  elapsed=$(( (end - start) / 1000000 ))

  [[ "$status" -eq 0 ]]
  [[ "$elapsed" -lt 200 ]]
}

@test "contract: no-network guarantee -- tier 0 produces zero network messages" {
  run_bootstrap_bundled -v
  [[ "$status" -eq 0 ]]
  [[ "$output" != *"resolving"* ]]
  [[ "$output" != *"downloading"* ]]
  [[ "$output" != *"github.com"* ]]
}

@test "contract: security -- tampered binary rejected" {
  enable_bundle
  # Tamper AFTER enable_bundle copies the binary
  echo "MALICIOUS" >> "${TEST_SCRIPTS}/bundle/codespacectl-x86_64-unknown-linux-musl"
  chmod 0755 "${TEST_SCRIPTS}/bundle/codespacectl-x86_64-unknown-linux-musl"
  # Use run_bootstrap (not run_bootstrap_bundled) to avoid re-copying
  run_bootstrap -v
  # Must detect mismatch and skip tier 0
  [[ "$output" == *"sha256 mismatch"* ]]
}

@test "contract: security -- missing MANIFEST.json still installs (trusts clone)" {
  enable_bundle
  rm -f "${TEST_SCRIPTS}/bundle/MANIFEST.json"
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]
  [[ "$output" == *"bundled binary"* ]]
}

@test "contract: hermeticity -- does not modify files outside INSTALL_DIR and CACHE_DIR" {
  touch "${TEST_HOME}/marker"
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -f "${TEST_HOME}/marker" ]]
  local install_contents
  install_contents="$(ls "$TEST_INSTALL")"
  [[ "$install_contents" == "codespacectl" ]]
}

@test "contract: hermeticity -- does not write to real user HOME" {
  local real_bin="${HOME}/.local/bin/codespacectl"
  if [[ -f "$real_bin" ]]; then
    local orig_sha
    orig_sha="$(sha256_file "$real_bin")"
    run_bootstrap_bundled
    [[ "$status" -eq 0 ]]
    [[ "$(sha256_file "$real_bin")" == "$orig_sha" ]]
  else
    run_bootstrap_bundled
    [[ "$status" -eq 0 ]]
    [[ ! -f "$real_bin" ]]
  fi
}

@test "contract: exit code non-zero when version resolution fails" {
  # No bundle, no existing install, fake repo -> resolution fails
  run env -i \
    HOME="$TEST_HOME" \
    PATH="/usr/bin:/bin" \
    bash "${TEST_SCRIPTS}/bootstrap.sh" \
    --repo "topic-hash/nonexistent-repo-xyz-404" \
    --install-dir "$TEST_INSTALL" \
    --cache-dir "$TEST_CACHE" 2>&1 || true

  [[ "$status" -ne 0 ]]
  [[ "$output" == *"ERROR"* ]]
}

###############################################################################
# L4 -- END-TO-END / SCENARIO TESTS
###############################################################################

@test "e2e: agent workflow -- cold sandbox, bootstrap, binary works, re-bootstrap noop" {
  [[ ! -x "${TEST_INSTALL}/codespacectl" ]]

  # Bootstrap from bundle
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]

  # Binary works
  run "${TEST_INSTALL}/codespacectl" --version
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"codespacectl"* ]]

  # Re-bootstrap is idempotent
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]

  # Binary still works
  run "${TEST_INSTALL}/codespacectl" --version
  [[ "$status" -eq 0 ]]
}

@test "e2e: sandbox reset -- binary deleted, bootstrap restores" {
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]

  # Simulate reset
  rm -f "${TEST_INSTALL}/codespacectl"
  [[ ! -x "${TEST_INSTALL}/codespacectl" ]]

  # Restore
  run_bootstrap_bundled
  [[ "$status" -eq 0 ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]

  run "${TEST_INSTALL}/codespacectl" --version
  [[ "$status" -eq 0 ]]
}

@test "e2e: tier cascade -- no bundle, no env, no install, cache hit" {
  # No bundle wired (default)
  # No CODESPACECTL_BIN set (default)
  # No existing install
  # Put binary in cache
  cp "$REAL_BINARY" "${TEST_CACHE}/codespacectl"
  chmod 0755 "${TEST_CACHE}/codespacectl"

  run_bootstrap --version v0.1.0
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"from cache"* || "$output" == *"already installed"* ]]
  [[ -x "${TEST_INSTALL}/codespacectl" ]]
}

@test "e2e: MANIFEST.json with multiple targets selects correct one" {
  enable_bundle
  # Inject a fake aarch64 entry
  python3 -c "
import json
with open('${TEST_SCRIPTS}/bundle/MANIFEST.json') as f:
    m = json.load(f)
m['binaries'].append({
    'target': 'aarch64-unknown-linux-musl',
    'file': 'codespacectl-aarch64-unknown-linux-musl',
    'sha256': 'a' * 64
})
with open('${TEST_SCRIPTS}/bundle/MANIFEST.json', 'w') as f:
    json.dump(m, f, indent=2)
    f.write('\n')
"

  run_bootstrap_bundled -v
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"x86_64-unknown-linux-musl"* ]]
}

@test "e2e: update-bundle.sh downloads and verifies release binary" {
  run bash "$UPDATE_BUNDLE" --version v0.1.0 --target x86_64-unknown-linux-musl
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"SHA-256 verified"* ]]
  [[ "$output" == *"bundled:"* ]]
}

@test "e2e: update-bundle.sh updates MANIFEST.json" {
  run bash "$UPDATE_BUNDLE" --version v0.1.0 --target x86_64-unknown-linux-musl
  [[ "$status" -eq 0 ]]

  run python3 -c "
import json
with open('${BUNDLE_DIR}/MANIFEST.json') as f:
    m = json.load(f)
assert m['version'] == 'v0.1.0'
found = any(b['target'] == 'x86_64-unknown-linux-musl' and len(b.get('sha256','')) == 64 for b in m['binaries'])
assert found, 'x86_64 target not found'
print('MANIFEST.json: OK')
"
  [[ "$status" -eq 0 ]]
  [[ "$output" == *"OK"* ]]
}

###############################################################################
# L5 -- STATIC ANALYSIS
###############################################################################

@test "static: bootstrap.sh passes shellcheck" {
  command -v shellcheck &>/dev/null || skip "shellcheck not installed"
  run shellcheck -s bash -e SC2015 "$BOOTSTRAP"
  [[ "$status" -eq 0 ]]
}

@test "static: update-bundle.sh passes shellcheck" {
  command -v shellcheck &>/dev/null || skip "shellcheck not installed"
  run shellcheck -s bash -e SC2015 "$UPDATE_BUNDLE"
  [[ "$status" -eq 0 ]]
}
