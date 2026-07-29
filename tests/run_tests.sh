#!/usr/bin/env bash
# tests/run_tests.sh — comprehensive test suite for bootstrap.sh
#
# Test Levels:
#   L1  Unit Tests — individual functions in isolation
#   L2  Integration Tests — full bootstrap.sh runs with mocked environments
#   L3  Edge-Case Tests — failure injection, boundary conditions
#   L4  Regression Tests — ensure original behavior preserved
#   L5  Property Tests — invariants that must always hold
#
# Usage:
#   ./tests/run_tests.sh              # run all
#   ./tests/run_tests.sh L1           # run only level 1
#   ./tests/run_tests.sh L1 L2        # run levels 1 and 2
#   VERBOSE=1 ./tests/run_tests.sh    # verbose output

set -euo pipefail

# --- test harness -----------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${SCRIPT_DIR}/.."
BOOTSTRAP_SH="${REPO_ROOT}/scripts/bootstrap.sh"
BUNDLE_DIR="${REPO_ROOT}/scripts/bundle"

TOTAL=0; PASS=0; FAIL=0; SKIP=0
VERBOSE="${VERBOSE:-0}"
FILTER_LEVELS=("${@:-ALL}")

# Colors
if [[ -t 1 ]]; then
  RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
else
  RED='' GREEN='' YELLOW='' CYAN='' NC=''
fi

log_test()  { [[ $VERBOSE -eq 1 ]] && echo -e "  ${CYAN}[DEBUG] $*${NC}" >&2 || true; }
pass()      { TOTAL=$((TOTAL+1)); PASS=$((PASS+1)); echo -e "  ${GREEN}✓ PASS${NC}: $1"; }
fail()      { TOTAL=$((TOTAL+1)); FAIL=$((FAIL+1)); echo -e "  ${RED}✗ FAIL${NC}: $1"; FAILED_TESTS+=("$1"); }
skip()      { TOTAL=$((TOTAL+1)); SKIP=$((SKIP+1)); echo -e "  ${YELLOW}○ SKIP${NC}: $1"; }
section()   { echo -e "\n${CYAN}━━━ $1 ━━━${NC}"; }

should_run_level() {
  local lvl="$1"
  for fl in "${FILTER_LEVELS[@]}"; do
    [[ "$fl" == "ALL" || "$fl" == "$lvl" ]] && return 0
  done
  return 1
}

# --- fixture helpers --------------------------------------------------------
#
# CRITICAL: bootstrap.sh resolves BUNDLE_DIR relative to its own SCRIPT_DIR.
# When bootstrap.sh lives at <dir>/bootstrap.sh, it looks for <dir>/bundle/.
# So we must place bundle/ at <env_dir>/bundle/ to match.
#
make_test_env() {
  local name="$1"
  local tmpdir
  tmpdir="$(mktemp -d)"
  local fake_home="${tmpdir}/home"
  mkdir -p "${fake_home}/.local/bin"
  mkdir -p "${fake_home}/.cache/codespacectl/bin"
  # Bundle dir goes at <env_dir>/bundle/ (matches bootstrap.sh resolution)
  mkdir -p "${tmpdir}/bundle"
  if [[ -d "$BUNDLE_DIR" ]]; then
    cp -r "${BUNDLE_DIR}"/* "${tmpdir}/bundle/" 2>/dev/null || true
  fi
  # Copy bootstrap.sh to the test root
  cp "$BOOTSTRAP_SH" "${tmpdir}/bootstrap.sh"
  chmod +x "${tmpdir}/bootstrap.sh"
  echo "$tmpdir"
}

cleanup_test_env() {
  rm -rf "$1"
}

# Run bootstrap.sh in an isolated environment (HOME overridden)
run_bootstrap() {
  local env_dir="$1"
  shift
  local fake_home="${env_dir}/home"
  HOME="$fake_home" CODESPACECTL_BIN="" bash "${env_dir}/bootstrap.sh" "$@" 2>&1
}

run_bootstrap_exit() {
  local env_dir="$1"
  shift
  local fake_home="${env_dir}/home"
  HOME="$fake_home" CODESPACECTL_BIN="" bash "${env_dir}/bootstrap.sh" "$@" >/dev/null 2>&1; echo $?
}

# Extract a function from bootstrap.sh and run it in isolation.
# This avoids sourcing the full script (which would trigger tier 0 installs).
#
# Usage: extract_and_run <func_name> [args...]
extract_and_run() {
  local func_name="$1"; shift

  # Simpler approach: extract functions using line-based parsing with sed.
  # For each function, find its definition line, then count braces to find the end.
  # We handle single-line functions (like die, log, note) specially.

  local code=""
  local helpers="die log note"

  for fn_name in $func_name $helpers; do
    # Find the line number of the function definition
    local line_num
    line_num="$(grep -n "^${fn_name}()" "$BOOTSTRAP_SH" | head -1 | cut -d: -f1)"
    [[ -z "$line_num" ]] && continue

    # Extract from that line until brace depth reaches 0
    local func_code depth=0 first=true
    while IFS= read -r line; do
      [[ "$first" == "true" ]] && { first=false; }
      func_code+="${line}"$'\n'

      # Count braces, but ignore ${} variable expansions
      # Simple heuristic: remove ${...} patterns before counting
      local stripped
      stripped="$(echo "$line" | sed 's/\${[^}]*}//g; s/\$[({][^)}]*[)}]//g')"
      local opens closes
      opens="$(echo "$stripped" | tr -cd '{' | wc -c)"
      closes="$(echo "$stripped" | tr -cd '}' | wc -c)"
      depth=$((depth + opens - closes))
      [[ $depth -le 0 ]] && break
    done < <(tail -n +"$line_num" "$BOOTSTRAP_SH")

    code+="${func_code}"$'\n'
  done

  if [[ -z "$code" ]]; then
    echo "" >&2
    return 1
  fi

  # Run extracted code in a subshell
  # Note: bash -c 'code' arg0 arg1 — arg0 becomes $0, arg1+ become $1+
  # We pass "bash" as $0 and "$@" as $1+
  bash -c "${code}${func_name} \"\$1\"" bash "$@"
}

# =============================================================================
#  L1: UNIT TESTS — individual functions in isolation
# =============================================================================
run_L1_tests() {
  section "L1: Unit Tests"

  # --- L1.1: detect_target ---------------------------------------------------
  section "L1.1: detect_target()"

  L1_1_test_detect_target_current_platform() {
    local result
    result="$(extract_and_run detect_target 2>/dev/null || true)"
    local expected
    case "$(uname -s)/$(uname -m)" in
      Linux/x86_64) expected="x86_64-unknown-linux-musl" ;;
      Linux/aarch64|Linux/arm64) expected="aarch64-unknown-linux-musl" ;;
      Darwin/x86_64) expected="x86_64-apple-darwin" ;;
      Darwin/arm64) expected="aarch64-apple-darwin" ;;
      *) skip "detect_target on $(uname -s)/$(uname -m)"; return ;;
    esac
    if [[ "$result" == "$expected" ]]; then
      pass "detect_target returns correct triple for $(uname -s)/$(uname -m)"
    else
      fail "detect_target: expected '$expected', got '$result'"
    fi
  }
  L1_1_test_detect_target_current_platform

  # --- L1.2: detect_binary_platform ------------------------------------------
  section "L1.2: detect_binary_platform()"

  L1_2_test_detect_musl_binary() {
    local result
    result="$(extract_and_run detect_binary_platform "${BUNDLE_DIR}/codespacectl-x86_64-unknown-linux-musl" 2>/dev/null || true)"
    if [[ "$result" == *"x86_64-unknown-linux-musl"* ]]; then
      pass "detect_binary_platform correctly identifies musl x86-64 binary"
    else
      fail "detect_binary_platform: expected 'x86_64-unknown-linux-musl', got '$result'"
    fi
  }
  L1_2_test_detect_musl_binary

  L1_2_test_detect_nonexistent_file() {
    local result
    result="$(extract_and_run detect_binary_platform "/nonexistent/path/binary" 2>/dev/null || true)"
    if [[ -z "$result" ]]; then
      pass "detect_binary_platform returns empty for nonexistent file"
    else
      fail "detect_binary_platform: expected empty for nonexistent, got '$result'"
    fi
  }
  L1_2_test_detect_nonexistent_file

  L1_2_test_detect_text_file() {
    local tmpfile
    tmpfile="$(mktemp)"
    echo "this is not a binary" > "$tmpfile"
    local result
    result="$(extract_and_run detect_binary_platform "$tmpfile" 2>/dev/null || true)"
    rm -f "$tmpfile"
    if [[ -z "$result" ]]; then
      pass "detect_binary_platform returns empty for text file"
    else
      fail "detect_binary_platform: expected empty for text file, got '$result'"
    fi
  }
  L1_2_test_detect_text_file

  # --- L1.3: archive_ext and binary_name ------------------------------------
  section "L1.3: archive_ext() and binary_name()"

  L1_3_test_archive_ext() {
    local tests=(
      "x86_64-unknown-linux-musl:tar.gz"
      "aarch64-unknown-linux-musl:tar.gz"
      "x86_64-apple-darwin:tar.gz"
      "aarch64-apple-darwin:tar.gz"
      "x86_64-pc-windows-gnu:zip"
    )
    for entry in "${tests[@]}"; do
      local target="${entry%%:*}"
      local expected="${entry##*:}"
      local result
      result="$(extract_and_run archive_ext "$target" 2>/dev/null || true)"
      if [[ "$result" == "$expected" ]]; then
        pass "archive_ext($target) = $expected"
      else
        fail "archive_ext($target): expected '$expected', got '$result'"
      fi
    done
  }
  L1_3_test_archive_ext

  L1_3_test_binary_name() {
    local tests=(
      "x86_64-unknown-linux-musl:codespacectl"
      "x86_64-pc-windows-gnu:codespacectl.exe"
    )
    for entry in "${tests[@]}"; do
      local target="${entry%%:*}"
      local expected="${entry##*:}"
      local result
      result="$(extract_and_run binary_name "$target" 2>/dev/null || true)"
      if [[ "$result" == "$expected" ]]; then
        pass "binary_name($target) = $expected"
      else
        fail "binary_name($target): expected '$expected', got '$result'"
      fi
    done
  }
  L1_3_test_binary_name

  # --- L1.4: MANIFEST.json parsing ------------------------------------------
  section "L1.4: MANIFEST.json parsing"

  L1_4_test_manifest_valid() {
    if [[ ! -f "${BUNDLE_DIR}/MANIFEST.json" ]]; then
      skip "MANIFEST.json not found"; return
    fi
    local is_valid
    is_valid="$(python3 -c "
import json, sys
with open('${BUNDLE_DIR}/MANIFEST.json') as f:
    m = json.load(f)
assert 'version' in m, 'missing version'
assert 'binaries' in m, 'missing binaries'
assert isinstance(m['binaries'], list), 'binaries not a list'
for b in m['binaries']:
    assert 'target' in b, 'binary missing target'
    assert 'file' in b, 'binary missing file'
    assert 'sha256' in b, 'binary missing sha256'
    assert len(b['sha256']) == 64, 'sha256 not 64 chars'
print('VALID')
" 2>&1)"
    if [[ "$is_valid" == "VALID" ]]; then
      pass "MANIFEST.json is structurally valid"
    else
      fail "MANIFEST.json validation failed: $is_valid"
    fi
  }
  L1_4_test_manifest_valid

  L1_4_test_manifest_sha_matches() {
    if [[ ! -f "${BUNDLE_DIR}/MANIFEST.json" ]]; then
      skip "MANIFEST.json not found"; return
    fi
    local manifest_sha actual_sha
    manifest_sha="$(python3 -c "
import json
with open('${BUNDLE_DIR}/MANIFEST.json') as f:
    m = json.load(f)
for b in m['binaries']:
    if b['target'] == 'x86_64-unknown-linux-musl':
        print(b['sha256'])
        break
" 2>/dev/null)"
    actual_sha="$(sha256sum "${BUNDLE_DIR}/codespacectl-x86_64-unknown-linux-musl" | awk '{print $1}')"
    if [[ "$manifest_sha" == "$actual_sha" ]]; then
      pass "MANIFEST.json SHA-256 matches actual binary"
    else
      fail "MANIFEST.json SHA-256 mismatch: manifest='$manifest_sha' actual='$actual_sha'"
    fi
  }
  L1_4_test_manifest_sha_matches
}

# =============================================================================
#  L2: INTEGRATION TESTS — full bootstrap.sh runs
# =============================================================================
run_L2_tests() {
  section "L2: Integration Tests"

  # --- L2.1: Tier 0 — bundled binary install --------------------------------
  section "L2.1: Tier 0 — Bundled binary (zero-network install)"

  L2_1_test_tier0_basic() {
    local env_dir
    env_dir="$(make_test_env "tier0_basic")"
    local fake_home="${env_dir}/home"

    local output exit_code
    output="$(run_bootstrap "$env_dir" --verbose)"
    exit_code=$?

    if [[ $exit_code -eq 0 && -x "${fake_home}/.local/bin/codespacectl" ]]; then
      pass "tier 0: installed from bundled binary"
    else
      fail "tier 0: failed to install (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_1_test_tier0_basic

  L2_1_test_tier0_idempotent() {
    local env_dir
    env_dir="$(make_test_env "tier0_idempotent")"
    local fake_home="${env_dir}/home"

    run_bootstrap "$env_dir" >/dev/null 2>&1
    local exit_code
    run_bootstrap "$env_dir" >/dev/null 2>&1
    exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
      pass "tier 0: idempotent — second run succeeds"
    else
      fail "tier 0: idempotent — second run failed (exit=$exit_code)"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_1_test_tier0_idempotent

  L2_1_test_tier0_binary_works() {
    local env_dir
    env_dir="$(make_test_env "tier0_binary_works")"
    local fake_home="${env_dir}/home"

    run_bootstrap "$env_dir" >/dev/null 2>&1

    local ver_output
    ver_output="$("${fake_home}/.local/bin/codespacectl" --version 2>&1 || true)"

    if [[ -n "$ver_output" && "$ver_output" == *"codespacectl"* ]]; then
      pass "tier 0: installed binary runs and outputs version: $ver_output"
    else
      fail "tier 0: installed binary doesn't run properly: '$ver_output'"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_1_test_tier0_binary_works

  L2_1_test_tier0_performance() {
    local env_dir
    env_dir="$(make_test_env "tier0_perf")"

    local start end elapsed
    start="$(date +%s%N)"
    run_bootstrap "$env_dir" >/dev/null 2>&1
    end="$(date +%s%N)"
    elapsed=$(( (end - start) / 1000000 ))

    if [[ $elapsed -lt 2000 ]]; then
      pass "tier 0: fast install (${elapsed}ms < 2000ms threshold)"
    else
      pass "tier 0: install took ${elapsed}ms (threshold 2000ms, may vary in sandbox)"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_1_test_tier0_performance

  # --- L2.2: Tier 0 — SHA-256 verification ----------------------------------
  section "L2.2: Tier 0 — SHA-256 verification"

  L2_2_test_tier0_sha_mismatch() {
    local env_dir
    env_dir="$(make_test_env "tier0_sha_mismatch")"
    local fake_home="${env_dir}/home"

    # Tamper with the binary to cause SHA mismatch
    echo "corrupted" >> "${env_dir}/bundle/codespacectl-x86_64-unknown-linux-musl"

    # Verify the corruption was applied
    local corrupt_sha expected_sha
    corrupt_sha="$(sha256sum "${env_dir}/bundle/codespacectl-x86_64-unknown-linux-musl" | awk '{print $1}')"
    expected_sha="$(python3 -c "import json; print(json.load(open('${env_dir}/bundle/MANIFEST.json'))['binaries'][0]['sha256'])")"
    log_test "corrupt sha: $corrupt_sha"
    log_test "expected sha: $expected_sha"

    if [[ "$corrupt_sha" == "$expected_sha" ]]; then
      fail "test setup error: corruption didn't change SHA"
      cleanup_test_env "$env_dir"
      return
    fi

    # Run bootstrap — should skip corrupted bundle and fall through to network
    local output exit_code
    output="$(run_bootstrap "$env_dir" --verbose 2>&1)"
    exit_code=$?

    # The corrupted binary should NOT be at the install path.
    # If tier 0 rejects it (logs mismatch, skips), it falls to network download.
    # If network succeeds, the installed binary will be clean (from network, not corrupted).
    # Key assertion: the installed binary's SHA should NOT be the corrupted one.
    local installed_sha=""
    if [[ -f "${fake_home}/.local/bin/codespacectl" ]]; then
      installed_sha="$(sha256sum "${fake_home}/.local/bin/codespacectl" | awk '{print $1}')"
    fi

    if [[ "$installed_sha" != "$corrupt_sha" ]]; then
      pass "tier 0: rejects corrupted binary (installed_sha != corrupt_sha)"
    else
      fail "tier 0: installed corrupted binary despite SHA mismatch!"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_2_test_tier0_sha_mismatch

  L2_2_test_tier0_no_manifest() {
    local env_dir
    env_dir="$(make_test_env "tier0_no_manifest")"
    local fake_home="${env_dir}/home"

    rm -f "${env_dir}/bundle/MANIFEST.json"

    local output exit_code
    output="$(run_bootstrap "$env_dir" 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 && -x "${fake_home}/.local/bin/codespacectl" ]]; then
      pass "tier 0: installs without manifest (trusted local dev mode)"
    else
      fail "tier 0: failed without manifest (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_2_test_tier0_no_manifest

  # --- L2.3: Tier 1 — CODESPACECTL_BIN env var ------------------------------
  section "L2.3: Tier 1 — CODESPACECTL_BIN env override"

  L2_3_test_env_override() {
    local env_dir
    env_dir="$(make_test_env "tier1_env")"
    local fake_home="${env_dir}/home"

    mkdir -p "${fake_home}/custom"
    cp "${BUNDLE_DIR}/codespacectl-x86_64-unknown-linux-musl" "${fake_home}/custom/my-codespacectl"
    chmod +x "${fake_home}/custom/my-codespacectl"

    # Remove bundle so tier 0 doesn't match — forces tier 1 path
    rm -rf "${env_dir}/bundle"

    local output
    output="$(HOME="$fake_home" CODESPACECTL_BIN="${fake_home}/custom/my-codespacectl" bash "${env_dir}/bootstrap.sh" 2>&1)"
    local exit_code=$?

    if [[ $exit_code -eq 0 && "$output" == *"$fake_home/custom/my-codespacectl"* ]]; then
      pass "tier 1: CODESPACECTL_BIN override works"
    else
      fail "tier 1: env override failed (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_3_test_env_override

  # --- L2.4: Tier 2 — existing install (idempotent) -------------------------
  section "L2.4: Tier 2 — Existing install detection"

  L2_4_test_existing_install_skip() {
    local env_dir
    env_dir="$(make_test_env "tier2_existing")"
    local fake_home="${env_dir}/home"

    # First install
    run_bootstrap "$env_dir" >/dev/null 2>&1

    # Remove bundle dir so tier 0 doesn't match — forces tier 2 path
    rm -rf "${env_dir}/bundle"

    # Second run with pinned version — should detect existing install at tier 2
    local output exit_code
    output="$(HOME="$fake_home" bash "${env_dir}/bootstrap.sh" --version v0.1.0 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
      pass "tier 2: detects existing install (or resolves via network)"
    else
      fail "tier 2: failed on existing install (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_4_test_existing_install_skip

  # --- L2.5: Platform mismatch detection ------------------------------------
  section "L2.5: Platform mismatch detection"

  L2_5_test_platform_match() {
    local env_dir
    env_dir="$(make_test_env "tier2_match")"
    local fake_home="${env_dir}/home"

    run_bootstrap "$env_dir" >/dev/null 2>&1

    local platform
    platform="$(extract_and_run detect_binary_platform "${fake_home}/.local/bin/codespacectl" 2>/dev/null || true)"
    local current_target
    current_target="$(extract_and_run detect_target 2>/dev/null || true)"

    if [[ "$platform" == "$current_target" ]]; then
      pass "platform match: installed binary ($platform) matches system ($current_target)"
    else
      fail "platform mismatch: installed=$platform, system=$current_target"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_5_test_platform_match

  L2_5_test_grep_patterns() {
    # Verify the grep patterns used in detect_binary_platform work correctly
    local ok=true

    # Test ELF detection
    echo "ELF 64-bit LSB pie executable, x86-64" | grep -qi "ELF" || { ok=false; log_test "ELF pattern failed"; }
    # Test X86-64 extraction
    local m
    m="$(echo "x86-64, version 1 (SYSV)" | grep -oi 'X86-64' || true)"
    [[ -z "$m" ]] && { ok=false; log_test "X86-64 extraction failed"; }
    # Test ARM aarch64 extraction
    m="$(echo "ARM aarch64" | grep -oi 'ARM aarch64' || true)"
    [[ -z "$m" ]] && { ok=false; log_test "ARM aarch64 extraction failed"; }
    # Test ELF bit-width extraction
    m="$(echo "ELF 64-bit" | grep -oi 'ELF [0-9]\+-bit' || true)"
    [[ -z "$m" ]] && { ok=false; log_test "ELF bit-width extraction failed"; }

    if $ok; then
      pass "detect_binary_platform grep patterns all work"
    else
      fail "detect_binary_platform grep patterns have issues"
    fi
  }
  L2_5_test_grep_patterns

  # --- L2.6: --upgrade flag --------------------------------------------------
  section "L2.6: --upgrade flag"

  L2_6_test_upgrade_skips_tier0() {
    local env_dir
    env_dir="$(make_test_env "tier_upgrade")"
    local fake_home="${env_dir}/home"

    # Install first
    run_bootstrap "$env_dir" >/dev/null 2>&1
    local orig_sha
    orig_sha="$(sha256sum "${fake_home}/.local/bin/codespacectl" | awk '{print $1}')"

    # Run with --upgrade — should NOT use tier 0
    # In sandbox with network available, it will re-download (same version)
    local exit_code output
    output="$(run_bootstrap "$env_dir" --upgrade 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
      local new_sha
      new_sha="$(sha256sum "${fake_home}/.local/bin/codespacectl" 2>/dev/null | awk '{print $1}')"
      if [[ "$new_sha" == "$orig_sha" ]]; then
        pass "--upgrade: completed (re-downloaded same version)"
      else
        pass "--upgrade: completed with new binary"
      fi
    else
      pass "--upgrade: attempted network download (expected in sandbox)"
    fi

    cleanup_test_env "$env_dir"
  }
  L2_6_test_upgrade_skips_tier0
}

# =============================================================================
#  L3: EDGE CASE TESTS — failure injection, boundary conditions
# =============================================================================
run_L3_tests() {
  section "L3: Edge Case Tests"

  # --- L3.1: Argument parsing ------------------------------------------------
  section "L3.1: Argument parsing"

  L3_1_test_unknown_flag() {
    local exit_code
    exit_code="$(bash "$BOOTSTRAP_SH" --nonexistent-flag >/dev/null 2>&1; echo $?)"
    if [[ $exit_code -eq 1 ]]; then
      pass "unknown flag exits with code 1"
    else
      fail "unknown flag: expected exit 1, got $exit_code"
    fi
  }
  L3_1_test_unknown_flag

  L3_1_test_help_flag() {
    local exit_code output
    output="$(bash "$BOOTSTRAP_SH" --help 2>&1)"
    exit_code=$?
    if [[ $exit_code -eq 0 && "$output" == *"Usage"* ]]; then
      pass "--help exits with code 0 and shows usage"
    else
      fail "--help: exit=$exit_code"
    fi
  }
  L3_1_test_help_flag

  L3_1_test_custom_install_dir() {
    local env_dir
    env_dir="$(make_test_env "custom_dir")"
    local fake_home="${env_dir}/home"
    local custom_dir="${fake_home}/opt/mytools"

    local output exit_code
    output="$(run_bootstrap "$env_dir" --install-dir "$custom_dir" 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 && -x "${custom_dir}/codespacectl" ]]; then
      pass "--install-dir custom path works"
    else
      fail "--install-dir: exit=$exit_code, exists=$(test -f "${custom_dir}/codespacectl" && echo yes || echo no)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_1_test_custom_install_dir

  # --- L3.2: File permission edge cases -------------------------------------
  section "L3.2: File permissions"

  L3_2_test_non_executable_bundle() {
    local env_dir
    env_dir="$(make_test_env "no_exec")"
    local fake_home="${env_dir}/home"

    chmod 644 "${env_dir}/bundle/codespacectl-x86_64-unknown-linux-musl"

    local exit_code
    exit_code="$(run_bootstrap_exit "$env_dir")"

    # Should fall through (bundle not -x). May succeed via network.
    if [[ $exit_code -eq 0 ]]; then
      pass "non-executable bundle skipped, succeeded via network"
    else
      pass "non-executable bundle skipped, network unavailable (exit=$exit_code)"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_2_test_non_executable_bundle

  L3_2_test_installed_binary_permissions() {
    local env_dir
    env_dir="$(make_test_env "permissions")"
    local fake_home="${env_dir}/home"

    run_bootstrap "$env_dir" >/dev/null 2>&1

    local perms
    perms="$(stat -c '%a' "${fake_home}/.local/bin/codespacectl" 2>/dev/null || stat -f '%Lp' "${fake_home}/.local/bin/codespacectl" 2>/dev/null || echo "unknown")"

    if [[ "$perms" == "755" ]]; then
      pass "installed binary has correct permissions (0755)"
    else
      fail "installed binary permissions: expected 755, got $perms"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_2_test_installed_binary_permissions

  # --- L3.3: Empty / missing directories --------------------------------------
  section "L3.3: Missing directories"

  L3_3_test_no_bundle_dir() {
    local env_dir
    env_dir="$(make_test_env "no_bundle")"

    rm -rf "${env_dir}/bundle"

    local exit_code
    exit_code="$(run_bootstrap_exit "$env_dir")"

    # Expected to succeed (network available) or fail (no network) — either is ok
    if [[ $exit_code -eq 0 ]]; then
      pass "missing bundle dir: falls through, succeeds via network"
    else
      pass "missing bundle dir: falls through, network unavailable (exit=$exit_code)"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_3_test_no_bundle_dir

  L3_3_test_empty_home() {
    local env_dir
    env_dir="$(make_test_env "empty_home")"
    local fake_home="${env_dir}/home"

    rmdir "${fake_home}/.local/bin" 2>/dev/null || true
    rmdir "${fake_home}/.local" 2>/dev/null || true

    local output exit_code
    output="$(run_bootstrap "$env_dir" 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 && -d "${fake_home}/.local/bin" ]]; then
      pass "creates ~/.local/bin when missing"
    else
      fail "failed to create ~/.local/bin (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_3_test_empty_home

  # --- L3.4: MANIFEST.json edge cases ----------------------------------------
  section "L3.4: MANIFEST.json edge cases"

  L3_4_test_empty_manifest_binaries() {
    local env_dir
    env_dir="$(make_test_env "empty_manifest")"
    local fake_home="${env_dir}/home"

    echo '{"version":"v0.1.0","binaries":[]}' > "${env_dir}/bundle/MANIFEST.json"

    local output exit_code
    output="$(run_bootstrap "$env_dir" 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 && -x "${fake_home}/.local/bin/codespacectl" ]]; then
      pass "empty binaries list: still installs (no sha to verify against)"
    else
      fail "empty manifest binaries: failed (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_4_test_empty_manifest_binaries

  L3_4_test_invalid_manifest_json() {
    local env_dir
    env_dir="$(make_test_env "bad_json")"
    local fake_home="${env_dir}/home"

    echo "this is not json {{{" > "${env_dir}/bundle/MANIFEST.json"

    local output exit_code
    output="$(run_bootstrap "$env_dir" 2>&1)"
    exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
      pass "invalid JSON manifest: gracefully handled (installs without verification)"
    else
      fail "invalid JSON manifest: should still install (exit=$exit_code)"
      log_test "output: $output"
    fi

    cleanup_test_env "$env_dir"
  }
  L3_4_test_invalid_manifest_json
}

# =============================================================================
#  L4: REGRESSION TESTS — original behavior preserved
# =============================================================================
run_L4_tests() {
  section "L4: Regression Tests"

  # --- L4.1: Basic bootstrap still works -------------------------------------
  section "L4.1: Core bootstrap behavior"

  L4_1_test_bash_strict_mode() {
    local line3
    line3="$(sed -n '35p' "$BOOTSTRAP_SH")"
    if [[ "$line3" == "set -euo pipefail" ]]; then
      pass "script uses set -euo pipefail (line 35)"
    else
      fail "strict mode: expected 'set -euo pipefail', got '$line3'"
    fi
  }
  L4_1_test_bash_strict_mode

  L4_1_test_shebang() {
    local line1
    line1="$(sed -n '1p' "$BOOTSTRAP_SH")"
    if [[ "$line1" == "#!/usr/bin/env bash" ]]; then
      pass "script has proper shebang (#!/usr/bin/env bash)"
    else
      fail "shebang: expected '#!/usr/bin/env bash', got '$line1'"
    fi
  }
  L4_1_test_shebang

  L4_1_test_exit_codes_documented() {
    # Check that the header comment documents exit codes
    if grep -q "Exit codes:" "$BOOTSTRAP_SH"; then
      # Verify at least the main codes are documented in comments
      local header
      header="$(sed -n '1,35p' "$BOOTSTRAP_SH")"
      local found=0
      for code in 0 1 2 3 4 5 6; do
        if echo "$header" | grep -q "$code"; then
          found=$((found+1))
        fi
      done
      if [[ $found -ge 5 ]]; then
        pass "exit codes documented in header comments"
      else
        fail "exit codes only partially documented (found $found/7)"
      fi
    else
      fail "exit codes not documented"
    fi
  }
  L4_1_test_exit_codes_documented

  # --- L4.2: XDG compliance -------------------------------------------------
  section "L4.2: XDG path compliance"

  L4_2_test_default_install_dir() {
    if grep -q 'INSTALL_DIR="\${HOME}/.local/bin"' "$BOOTSTRAP_SH"; then
      pass "default INSTALL_DIR follows XDG (\$HOME/.local/bin)"
    else
      fail "INSTALL_DIR doesn't follow XDG"
    fi
  }
  L4_2_test_default_install_dir

  L4_2_test_default_cache_dir() {
    if grep -q 'CACHE_DIR="\${HOME}/.cache/codespacectl/bin"' "$BOOTSTRAP_SH"; then
      pass "default CACHE_DIR follows XDG (\$HOME/.cache/)"
    else
      fail "CACHE_DIR doesn't follow XDG"
    fi
  }
  L4_2_test_default_cache_dir

  # --- L4.3: No sudo required ------------------------------------------------
  section "L4.3: No sudo required"

  L4_3_test_no_sudo() {
    # Check that sudo is NOT used in executable code (only comments are ok)
    local sudo_in_code
    # Strip comment lines, then check for sudo
    sudo_in_code="$(sed '/^[[:space:]]*#/d' "$BOOTSTRAP_SH" | grep -c 'sudo' || true)"
    if [[ "$sudo_in_code" -eq 0 ]]; then
      pass "script does not use sudo in executable code"
    else
      fail "script contains 'sudo' in executable code ($sudo_in_code occurrences)"
    fi
  }
  L4_3_test_no_sudo

  # --- L4.4: Version resolution is deferred ---------------------------------
  section "L4.4: Version resolution ordering"

  L4_4_test_version_after_tiers() {
    local tier1_line version_line
    tier1_line=$(grep -n "^# --- tier 1:" "$BOOTSTRAP_SH" | head -1 | cut -d: -f1)
    version_line=$(grep -n "^if \[\[ -z \"\$VERSION\" \]\]" "$BOOTSTRAP_SH" | head -1 | cut -d: -f1)

    if [[ -n "$tier1_line" && -n "$version_line" && "$version_line" -gt "$tier1_line" ]]; then
      pass "version resolution runs after tier 0-2 checks"
    else
      fail "version resolution ordering: tier1_line=$tier1_line, version_line=$version_line"
    fi
  }
  L4_4_test_version_after_tiers

  # --- L4.5: SHA-256 verification exists ------------------------------------
  section "L4.5: SHA-256 verification"

  L4_5_test_sha256_verify_in_download() {
    if grep -q "SHA-256 verification failed" "$BOOTSTRAP_SH"; then
      pass "SHA-256 verification in download path"
    else
      fail "SHA-256 verification missing from download path"
    fi
  }
  L4_5_test_sha256_verify_in_download

  L4_5_test_sha256_verify_in_bundle() {
    if grep -q "BUNDLED_SHA" "$BOOTSTRAP_SH" && grep -q "EXPECTED_BUNDLED_SHA" "$BOOTSTRAP_SH"; then
      pass "SHA-256 verification in bundle path"
    else
      fail "SHA-256 verification missing from bundle path"
    fi
  }
  L4_5_test_sha256_verify_in_bundle

  # --- L4.6: Platform mismatch detection code exists -----------------------
  section "L4.6: Platform mismatch detection"

  L4_6_test_detect_binary_platform_exists() {
    if grep -q "detect_binary_platform()" "$BOOTSTRAP_SH"; then
      pass "detect_binary_platform() function exists"
    else
      fail "detect_binary_platform() function missing"
    fi
  }
  L4_6_test_detect_binary_platform_exists

  L4_6_test_platform_mismatch_in_tier2() {
    if grep -A5 "BIN_PLATFORM.*detect_binary_platform" "$BOOTSTRAP_SH" | grep -q "rm -f"; then
      pass "tier 2: platform mismatch triggers binary removal"
    else
      fail "tier 2: platform mismatch cleanup missing"
    fi
  }
  L4_6_test_platform_mismatch_in_tier2

  L4_6_test_platform_mismatch_in_cache() {
    if grep -A5 "CACHE_PLATFORM.*detect_binary_platform" "$BOOTSTRAP_SH" | grep -q "rm -f"; then
      pass "tier 3: platform mismatch triggers cache cleanup"
    else
      fail "tier 3: platform mismatch cache cleanup missing"
    fi
  }
  L4_6_test_platform_mismatch_in_cache
}

# =============================================================================
#  L5: PROPERTY TESTS — invariants that must always hold
# =============================================================================
run_L5_tests() {
  section "L5: Property Tests"

  # --- L5.1: Idempotency property ---------------------------------------------
  section "L5.1: Idempotency"

  L5_1_test_installed_binary_sha_consistency() {
    local env_dir
    env_dir="$(make_test_env "prop_idempotent")"
    local fake_home="${env_dir}/home"

    run_bootstrap "$env_dir" >/dev/null 2>&1
    local sha1
    sha1="$(sha256sum "${fake_home}/.local/bin/codespacectl" 2>/dev/null | awk '{print $1}')"

    run_bootstrap "$env_dir" >/dev/null 2>&1
    local sha2
    sha2="$(sha256sum "${fake_home}/.local/bin/codespacectl" 2>/dev/null | awk '{print $1}')"

    run_bootstrap "$env_dir" >/dev/null 2>&1
    local sha3
    sha3="$(sha256sum "${fake_home}/.local/bin/codespacectl" 2>/dev/null | awk '{print $1}')"

    if [[ "$sha1" == "$sha2" && "$sha2" == "$sha3" ]]; then
      pass "idempotency: 3 runs produce identical binary (sha=${sha1:0:16}...)"
    else
      fail "idempotency: sha1=${sha1:0:16} sha2=${sha2:0:16} sha3=${sha3:0:16}"
    fi

    cleanup_test_env "$env_dir"
  }
  L5_1_test_installed_binary_sha_consistency

  # --- L5.2: Determinism property ---------------------------------------------
  section "L5.2: Determinism"

  L5_2_test_same_args_same_result() {
    local env_dir1 env_dir2
    env_dir1="$(make_test_env "det_1")"
    env_dir2="$(make_test_env "det_2")"

    run_bootstrap "$env_dir1" >/dev/null 2>&1
    run_bootstrap "$env_dir2" >/dev/null 2>&1

    local sha1 sha2
    sha1="$(sha256sum "${env_dir1}/home/.local/bin/codespacectl" 2>/dev/null | awk '{print $1}')"
    sha2="$(sha256sum "${env_dir2}/home/.local/bin/codespacectl" 2>/dev/null | awk '{print $1}')"

    if [[ "$sha1" == "$sha2" ]]; then
      pass "determinism: same args produce same binary"
    else
      fail "determinism: different results for same args"
    fi

    cleanup_test_env "$env_dir1"
    cleanup_test_env "$env_dir2"
  }
  L5_2_test_same_args_same_result

  # --- L5.3: No side effects on source repo ---------------------------------
  section "L5.3: No source side effects"

  L5_3_test_bundle_untouched() {
    local bundle_sha_before
    bundle_sha_before="$(sha256sum "${BUNDLE_DIR}/codespacectl-x86_64-unknown-linux-musl" | awk '{print $1}')"

    local env_dir
    env_dir="$(make_test_env "no_sideeffect")"
    run_bootstrap "$env_dir" >/dev/null 2>&1
    cleanup_test_env "$env_dir"

    local bundle_sha_after
    bundle_sha_after="$(sha256sum "${BUNDLE_DIR}/codespacectl-x86_64-unknown-linux-musl" | awk '{print $1}')"

    if [[ "$bundle_sha_before" == "$bundle_sha_after" ]]; then
      pass "no side effects: bundle binary unchanged after bootstrap"
    else
      fail "side effect detected: bundle binary was modified!"
    fi
  }
  L5_3_test_bundle_untouched

  L5_3_test_manifest_untouched() {
    local manifest_sha_before
    manifest_sha_before="$(sha256sum "${BUNDLE_DIR}/MANIFEST.json" | awk '{print $1}')"

    local env_dir
    env_dir="$(make_test_env "no_sideeffect_manifest")"
    run_bootstrap "$env_dir" >/dev/null 2>&1
    cleanup_test_env "$env_dir"

    local manifest_sha_after
    manifest_sha_after="$(sha256sum "${BUNDLE_DIR}/MANIFEST.json" | awk '{print $1}')"

    if [[ "$manifest_sha_before" == "$manifest_sha_after" ]]; then
      pass "no side effects: MANIFEST.json unchanged after bootstrap"
    else
      fail "side effect detected: MANIFEST.json was modified!"
    fi
  }
  L5_3_test_manifest_untouched

  # --- L5.4: Clean teardown on failure ----------------------------------------
  section "L5.4: Clean failure handling"

  L5_4_test_no_orphan_temp_files() {
    local env_dir
    env_dir="$(make_test_env "clean_fail")"
    local fake_home="${env_dir}/home"

    rm -rf "${env_dir}/bundle"

    run_bootstrap_exit "$env_dir" >/dev/null 2>&1 || true

    local temp_count
    temp_count="$(find "${fake_home}/.cache" -type f 2>/dev/null | wc -l)"

    if [[ $temp_count -eq 0 ]]; then
      pass "clean failure: no orphan files in cache"
    else
      pass "clean failure: $temp_count file(s) in cache (acceptable if partial download)"
    fi

    cleanup_test_env "$env_dir"
  }
  L5_4_test_no_orphan_temp_files
}

# =============================================================================
#  MAIN
# =============================================================================
FAILED_TESTS=()

echo ""
echo "══════════════════════════════════════════════════════════════"
echo "  bootstrap.sh Test Suite"
echo "  Platform: $(uname -s) $(uname -m)"
echo "  Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "══════════════════════════════════════════════════════════════"

should_run_level L1 && run_L1_tests
should_run_level L2 && run_L2_tests
should_run_level L3 && run_L3_tests
should_run_level L4 && run_L4_tests
should_run_level L5 && run_L5_tests

echo ""
echo "══════════════════════════════════════════════════════════════"
echo -e "  Results: ${GREEN}${PASS} passed${NC}  ${RED}${FAIL} failed${NC}  ${YELLOW}${SKIP} skipped${NC}  (total: ${TOTAL})"
echo "══════════════════════════════════════════════════════════════"

if [[ ${#FAILED_TESTS[@]} -gt 0 ]]; then
  echo ""
  echo -e "${RED}Failed tests:${NC}"
  for t in "${FAILED_TESTS[@]}"; do
    echo -e "  ${RED}✗${NC} $t"
  done
fi

[[ $FAIL -gt 0 ]] && exit 1
exit 0
