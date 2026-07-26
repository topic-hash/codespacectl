# Setting Up CI/Release Workflows

The `codespacectl` repository's CI/release GitHub Actions workflow files
(`.github/workflows/ci.yml` and `.github/workflows/release.yml`) cannot be pushed
via a Personal Access Token that lacks the `workflow` scope — GitHub rejects the
push with:

```
! [remote rejected] main -> main (refusing to allow a Personal Access Token
  to create or update workflow `.github/workflows/ci.yml` without `workflow` scope)
```

This is a security restriction: workflow files can execute arbitrary code on
GitHub-hosted runners, so they require the `workflow` scope to push.

## Two ways to set up the workflows

### Option A: Regenerate the PAT with `workflow` scope (recommended)

1. Go to GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens
2. Either regenerate the existing token with the `workflow` scope added, or create a new token with:
   - `codespace` scope (for codespace management)
   - `repo` scope (for repo contents)
   - `workflow` scope (for GitHub Actions files) ← **this is the missing scope**
3. Re-export the new token: `export GH_TOKEN=ghp_xxx`
4. Clone the repo, add the workflow files (see below for content), commit, push.

### Option B: Paste via GitHub web UI (no token change needed)

1. Go to https://github.com/topic-hash/codespacectl/actions/new
2. Click "set up a workflow yourself"
3. Delete the default `blank.yml` content
4. Paste the contents of `ci.yml` (below)
5. Name the file `ci.yml` and commit
6. Repeat for `release.yml`

## Workflow file contents (current — Rust 1.85 MSRV + verification job)

### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

jobs:
  test:
    name: Test (stable)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: Format check
        run: cargo fmt --check
      - name: Clippy (advisory)
        continue-on-error: true
        run: cargo clippy -- -D warnings
      - name: Build
        run: cargo build --release --locked
      - name: Unit tests
        run: cargo test --lib -- --test-threads=1
      - name: Integration tests
        run: cargo test --test '*'

  msrv:
    name: MSRV (1.85)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85.0
      - uses: Swatinem/rust-cache@v2
        with:
          key: msrv-1.85
      - name: Build on MSRV
        run: cargo +1.85.0 build --release --locked
      - name: Test on MSRV
        run: cargo +1.85.0 test --lib -- --test-threads=1

```

### `.github/workflows/release.yml`

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-musl
            os: ubuntu-latest
            archive: tar.gz
          - target: aarch64-unknown-linux-musl
            os: ubuntu-latest
            archive: tar.gz
          - target: x86_64-apple-darwin
            os: macos-latest
            archive: tar.gz
          - target: aarch64-apple-darwin
            os: macos-latest
            archive: tar.gz
          - target: x86_64-pc-windows-gnu
            os: ubuntu-latest
            archive: zip
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85.0
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
      - name: Install musl tools (Linux x86_64)
        if: matrix.target == 'x86_64-unknown-linux-musl'
        run: sudo apt-get update && sudo apt-get install -y musl-tools
      - name: Install aarch64 cross (Linux arm64)
        if: matrix.target == 'aarch64-unknown-linux-musl'
        run: |
          sudo apt-get update
          sudo apt-get install -y gcc-aarch64-linux-gnu
          echo "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc" >> $GITHUB_ENV
      - name: Install mingw (Windows cross)
        if: matrix.target == 'x86_64-pc-windows-gnu'
        run: sudo apt-get update && sudo apt-get install -y gcc-mingw-w64-x86-64
      - name: Build
        run: cargo +1.85.0 build --release --locked --target ${{ matrix.target }}
      - name: Strip (non-Windows)
        if: matrix.archive != 'zip'
        run: strip target/${{ matrix.target }}/release/codespacectl
      - name: Package (tar.gz)
        if: matrix.archive == 'tar.gz'
        run: |
          cd target/${{ matrix.target }}/release
          tar czf codespacectl-${{ github.ref_name }}-${{ matrix.target }}.tar.gz codespacectl
          sha256sum codespacectl-${{ github.ref_name }}-${{ matrix.target }}.tar.gz > codespacectl-${{ github.ref_name }}-${{ matrix.target }}.tar.gz.sha256
      - name: Package (zip)
        if: matrix.archive == 'zip'
        run: |
          cd target/${{ matrix.target }}/release
          zip codespacectl-${{ github.ref_name }}-${{ matrix.target }}.zip codespacectl.exe
          sha256sum codespacectl-${{ github.ref_name }}-${{ matrix.target }}.zip > codespacectl-${{ github.ref_name }}-${{ matrix.target }}.zip.sha256
      - uses: actions/upload-artifact@v4
        with:
          name: codespacectl-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/codespacectl-${{ github.ref_name }}-${{ matrix.target }}.*

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
      - name: Merge SHA256SUMS
        run: |
          cd artifacts
          cat */*.sha256 > SHA256SUMS.txt
          rm */*.sha256
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          fail_on_unmatched_files: true
          files: |
            artifacts/*/codespacectl-*.tar.gz
            artifacts/*/codespacectl-*.zip
            artifacts/SHA256SUMS.txt

```

## What the workflows do

### `ci.yml` — two parallel jobs

**`test` job** (runs on every push/PR to main, uses Rust stable):
- Format check: `cargo fmt --check`
- Clippy (advisory, continue-on-error): `cargo clippy -- -D warnings`
- Build: `cargo build --release --locked` (verifies Cargo.lock is reproducible)
- Unit tests: `cargo test --lib -- --test-threads=1` (serialized because env-var-touching tests race otherwise)
- Integration tests: `cargo test --test '*'` (assert_cmd-based CLI smoke tests)

**`msrv` job** (runs on every push/PR to main, uses Rust 1.85.0):
- Build: `cargo +1.85.0 build --release --locked`
- Test: `cargo +1.85.0 test --lib -- --test-threads=1`
- Verifies that the declared MSRV in `Cargo.toml` (`rust-version = "1.85"`) is real, not aspirational.
- This catches the case where a transitive dependency bumps its MSRV above ours.

### `release.yml` (runs on tag push matching `v*`)

- 5-target cross-compile matrix using **Rust 1.85.0** (matches MSRV exactly):
  - `x86_64-unknown-linux-musl` (static Linux x86_64)
  - `aarch64-unknown-linux-musl` (static Linux ARM64)
  - `x86_64-apple-darwin` (macOS Intel)
  - `aarch64-apple-darwin` (macOS ARM/Apple Silicon)
  - `x86_64-pc-windows-gnu` (Windows x86_64 via mingw cross)
- Per-target: install cross-compile toolchain, build `--release --locked`, strip (non-Windows), package as tar.gz or zip + SHA-256 sidecar
- Aggregate release job: merges all SHA-256s into `SHA256SUMS.txt`, creates GitHub Release with auto-generated notes

## After setup: triggering the first release

```bash
git tag -a v0.1.0 -m "First release"
git push origin v0.1.0
```

This triggers the `release.yml` workflow, which builds all 5 binaries and creates a GitHub Release at https://github.com/topic-hash/codespacectl/releases/tag/v0.1.0 with download links for each platform + the SHA256SUMS.txt file.

Users can then install via:
```bash
curl -L https://github.com/topic-hash/codespacectl/releases/download/v0.1.0/codespacectl-v0.1.0-x86_64-unknown-linux-musl.tar.gz \
  | tar xz -C /usr/local/bin/
```
