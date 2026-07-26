# codespacectl — Architecture

This document is the high-level architecture overview for `codespacectl`. It
covers the design philosophy, the data flow between modules, and the
implementation choices that distinguish `codespacectl` from ad-hoc shell +
Python scripts.

- **Status**: living document — should be updated alongside major refactors
- **Audience**: contributors and operators who want to know *why* the binary
  behaves the way it does, not just *what* the flags do

---

## 1. Design philosophy

`codespacectl` was built to replace ad-hoc Python scripts and 132-line agent
prompts with a single, predictable binary. Three constraints shape every
decision:

1. **No system SSH**. The binary must run in a fresh Codespaces dev container
   without `apt install openssh-client` or `ssh-keygen`. SSH is provided by
   `russh` (pure Rust) over a `gh cs ssh --stdio` subprocess.
2. **No daemon**. There's no server process to start, no port to manage, no
   background state to lose. Every invocation is a short-lived CLI process
   that loads state from disk, does its work, and writes state back.
3. **Agent-friendly**. Every command emits a stable JSON envelope
   (`codespacectl/v1`) so AI agents and scripts can parse output without
   regex. Errors come with `kind`, `retryable`, and `suggested_action` so
   the caller can decide whether to retry or escalate.

---

## 2. Single-binary layout

The release binary is one ~10 MB static-linked Rust executable
(`target/*/release/codespacectl`). There is no `codespacectl-server`, no
`codespacectl-agent`, no plugin loader. The workspace has a lib + bin split:

```
codespacectl/
├── Cargo.toml          [[bin]] src/main.rs    [[lib]] src/lib.rs
└── src/
    ├── main.rs         entry point — parses CLI, dispatches, prints envelope
    ├── lib.rs          re-exports public modules for embedding/testing
    ├── cli/
    │   ├── args.rs     clap-derived CLI schema
    │   ├── output.rs   OutputEnvelope<T>, SessionRef, print_envelope
    │   └── commands/   one handler per subcommand (12 of them)
    ├── manifest/       CODESPACE.yaml schema + parser + template renderer
    ├── state/          state.json load/save (atomic, 0600)
    ├── github/         reqwest client for Codespaces REST API + token auth
    ├── ssh/            russh over gh cs ssh --stdio subprocess
    ├── health/         manifest health check runner
    ├── exec/           manifest command exec + lifecycle hooks
    ├── secrets/        age-encrypted at-rest secret storage + generation
    ├── session/        NDJSON append-only session log
    └── error.rs        typed error enum (18 kinds) → JSON envelope
```

The lib/bin split means `cargo test --lib` exercises every module without
touching the OS (no live codespace, no real network), and the binary
`main.rs` is a thin dispatcher that wires CLI args → handler functions →
envelope printer.

---

## 3. Data flow

A typical `codespacectl exec test --json` call flows through the binary as
follows (heavily simplified — see source for the full picture):

```
                                 ┌────────────┐
   argv ────────────────────────►│ main.rs    │
   (clap parse)                  │ dispatch   │
                                 └─────┬──────┘
                                       │
                                       ▼
                          ┌────────────────────────┐
                          │ cli/commands/exec.rs   │
                          │ (async fn handle)      │
                          └──────────┬─────────────┘
                                     │
              ┌──────────────────────┼───────────────────────┐
              ▼                      ▼                       ▼
   ┌──────────────────┐   ┌──────────────────┐    ┌──────────────────┐
   │ github/auth.rs    │   │ manifest/        │    │ state/           │
   │ resolve_token     │   │ find_manifest    │    │ load_state       │
   │ + validate        │   │ parse_manifest   │    │ (current_cs)     │
   └────────┬─────────┘   └────────┬─────────┘    └────────┬─────────┘
            │                      │                       │
            ▼                      ▼                       │
   ┌──────────────────┐   ┌──────────────────┐             │
   │ github/client.rs │   │ secrets/         │             │
   │ (reqwest)        │   │ resolve_template │             │
   │ validate_token   │   │ _context          │             │
   └────────┬─────────┘   └────────┬─────────┘             │
            │                      │                       │
            └──────────┬───────────┴───────────────────────┘
                       │
                       ▼
              ┌──────────────────┐
              │ cli/common.rs    │
              │ resolve_gh_bin   │
              └────────┬─────────┘
                       │
                       ▼
              ┌──────────────────┐         ┌────────────────┐
              │ ssh/transport.rs │◄────────│ gh cs ssh      │
              │ CodespaceSsh::   │  stdin  │ --stdio        │
              │ connect + exec   │  stdout  │ (subprocess)   │
              └────────┬─────────┘         └────────────────┘
                       │
                       ▼
              ┌──────────────────┐
              │ exec/mod.rs     │
              │ exec_command    │──► session log (NDJSON)
              └────────┬─────────┘
                       │
                       ▼
              ┌──────────────────┐
              │ cli/output.rs    │
              │ OutputEnvelope  │──► stdout (JSON or text)
              │ + print_envelope │
              └──────────────────┘
```

The lib modules are pure (no I/O except where they own it: `state` owns the
state file, `secrets` owns the secret store, `ssh` owns the subprocess, etc).
The `cli/commands/*.rs` files are the only place that wires modules
together; tests can target each module in isolation.

---

## 4. SSH transport — russh over `gh cs ssh --stdio`

This is the most unusual design choice, so it deserves the most explanation.

### 4.1 The constraint

GitHub Codespaces exposes SSH access through a custom protocol: the `gh` CLI
runs `gh cs ssh -c <name> --stdio`, which proxies an SSH connection from the
client's stdin/stdout to the codespace over an internal tunnel. The client
is responsible for actually running SSH.

### 4.2 What most tools do

Most existing codespace automation either:

1. Shells out to `gh cs ssh -c <name> -- <command>` directly. This works but
   you lose structured output, per-command timeouts, exit-code propagation,
   and you have no way to do TOFU host-key verification.
2. Requires `openssh-client` to be installed (apt/brew/etc.), then runs
   `ssh` against the proxy. This violates our "no system SSH" constraint —
   fresh Codespaces containers don't have `ssh` until you install it.

### 4.3 What `codespacectl` does

We use [`russh`](https://crates.io/crates/russh) 0.46 — a pure-Rust SSH
client — and feed it the stdin/stdout pipes of the `gh cs ssh --stdio`
subprocess. Concretely (`src/ssh/transport.rs`):

1. Spawn `gh cs ssh -c <name> --stdio` with `tokio::process::Command`,
   piping stdin/stdout (stderr inherited).
2. Wrap the `ChildStdin` and `ChildStdout` in an `SshTransport` struct that
   implements `tokio::io::AsyncRead + AsyncWrite` (via safe pin projection
   — both halves are `Unpin`).
3. Pass that transport to `russh::client::connect_stream(config, transport,
   handler)` with a 30s connect timeout and 15s keepalive.
4. Generate or load an Ed25519 keypair at `~/.cache/codespacectl/id_codespace`
   (`0600` perms on Unix).
5. Authenticate via `russh::client::Handle::authenticate_publickey`.
6. The `ClientHandler::check_server_key` callback captures the host key
   fingerprint (for TOFU — see §7).
7. Each `exec` opens a session channel, calls `channel.exec(false, command)`,
   and drains `channel.wait()` for `Data`, `ExtendedData`, `ExitStatus`,
   `Eof`, `Close`.

### 4.4 Why this works

- **Zero system dependencies**: we ship `russh` statically linked. The only
  external process we need is `gh`, and we ship a copy of `gh` at
  `tools/bin/gh` so the binary works even on machines that don't have `gh`
  installed.
- **Structured output**: because we own the SSH session, we capture
  stdout/stderr/exit-code separately for each command and return them as
  `ExecOutput`.
- **Per-command timeouts**: `tokio::time::timeout` wraps the channel
  read loop; on timeout we kill the subprocess and return
  `CommandTimeout { timeout_secs }`.
- **TOFU host keys**: `check_server_key` lets us compute the fingerprint
  *before* accepting the key, so we can compare it against state and abort
  if it's unexpected.

---

## 5. The fake-ssh directory

### 5.1 The problem

Even though `gh cs ssh --stdio` doesn't actually use the `ssh` binary (it
proxies the protocol internally), the `gh` CLI performs a pre-flight check
at startup: it calls `exec.LookPath("ssh")` and `exec.LookPath("ssh-keygen")`
and refuses to start if either is missing. This means on a fresh Codespaces
container without `openssh-client` installed, `gh cs ssh --stdio` fails with
"could not find ssh executable".

### 5.2 The solution

`codespacectl` ships stub binaries at `tools/bin/fake-ssh/ssh` and
`tools/bin/fake-ssh/ssh-keygen`. They are bash scripts that:

- `ssh`: responds to `ssh -V` with a fake version string and exits 0.
  Otherwise just exits 0 without doing anything (it's never actually
  invoked — `gh` only calls `LookPath`).
- `ssh-keygen`: parses `-t ed25519 -N "" -f <path> -C <comment>` arguments
  and generates a real Ed25519 keypair using Python's `cryptography`
  library, writing OpenSSH-format PEM to `<path>` and `<path>.pub` with
  `0600` perms. This *is* invoked on first connect (because `russh-keys`
  needs an Ed25519 keypair and we delegate to `ssh-keygen` for the OpenSSH
  format).

### 5.3 How they're wired in

When `codespacectl` runs `gh cs ssh --stdio`, it sets the `PATH` env var to
prepend `tools/bin/fake-ssh/`. `gh`'s `LookPath` calls then succeed and it
starts normally; the SSH protocol is then handled entirely by `russh`
talking to `gh`'s stdin/stdout.

### 5.4 Why this is OK

- The stubs do **no real SSH work**. They exist only to satisfy `gh`'s
  pre-flight check.
- The `ssh-keygen` stub does generate a real keypair, but only when
  `codespacectl` itself asks it to (during first-connect setup of the
  Ed25519 client key). The host-key fingerprint verification happens in
  `russh`'s `check_server_key` callback, completely independent of the
  `ssh-keygen` stub.
- The stubs are pure bash + python3 — no Rust, no openssh, no compilation.

---

## 6. State file

### 6.1 What it tracks

`~/.cache/codespacectl/state.json` is the single source of truth across
invocations. It tracks:

- The current codespace name (so `exec`/`raw`/`health`/`stop` work without
  re-specifying `--codespace`).
- The current manifest path + SHA-256 (for change detection across runs).
- Per-codespace state: last known state, last checked time, codespace
  creation time (from GitHub API), SSH host key fingerprint, host key
  stored-at time, last health status + time.
- Per-manifest state: SHA-256 + last validated time.
- The token fingerprint (`sha256(token)[:8]`) — never the full token — so
  callers can detect a token swap without re-running `token set`.

### 6.2 Atomic writes

`save_state` (in `src/state/file.rs`) writes the state file atomically:

1. Serialize the state to pretty JSON.
2. Write to `state.json.tmp` (in the same directory).
3. `rename(state.json.tmp, state.json)` — atomic on Unix and Windows.
4. `chmod 0600` (Unix only).

This means a crash mid-write leaves the previous state intact; readers
either see the old or the new state, never a half-written file.

### 6.3 XDG paths

`state_dir()` uses `dirs::cache_dir()` which honors:

- Linux: `$XDG_CACHE_HOME` (default `~/.cache`)
- macOS: `~/Library/Caches`
- Windows: `%LOCALAPPDATA%`

So `XDG_CACHE_HOME=/tmp/cc cargo test --lib` is what the tests use to avoid
clobbering the developer's real state.

### 6.4 Permissions

The state file gets `0600` perms on Unix because it contains the token
fingerprint and the codespace host key fingerprints. Anyone with read
access to the state file could detect that *this* machine is talking to
*these* codespaces.

---

## 7. TOFU host key model

Trust On First Use, with one twist: **codespace rebuilds rotate the host
key**, and we want to detect that automatically instead of forcing the user
to pass `--accept-new-host-key`.

### 7.1 First connect

1. The `russh` `check_server_key` callback captures the incoming host key
   fingerprint (OpenSSH `SHA256:<base64-nopad>` format).
2. `state.codespaces[name].host_key_fingerprint` is `None` → decision is
   `StoreNew` → we store the fingerprint + `host_key_stored_at` timestamp
   and accept the connection.

### 7.2 Subsequent connects (codespace unchanged)

1. Capture incoming fingerprint.
2. `state.codespaces[name].host_key_fingerprint` is `Some(stored)`.
3. If `incoming == stored` → decision is `Match` → proceed silently.

### 7.3 Subsequent connects (codespace was rebuilt)

GitHub Codespaces regenerates the SSH host key every time a codespace is
rebuilt (the underlying VM is destroyed and a new one is provisioned).
We detect this via the `created_at` field from the GitHub Codespaces API:

1. On every `connect`, we store `state.codespaces[name].created_at` (from
   the API response).
2. On the next `connect`, if `state.codespaces[name].created_at` is newer
   than `state.codespaces[name].host_key_stored_at`, the codespace was
   rebuilt since we last stored the host key.
3. In that case (`CodespaceState::host_key_is_stale()` returns `true`),
   a mismatched host key is treated as `Rotate` rather than `Reject`:
   - With `--accept-new-host-key`: the new fingerprint replaces the old
     one, the connection proceeds.
   - Without `--accept-new-host-key`: `HostKeyMismatch` error is returned
     with both fingerprints, telling the operator to either pass the flag
     or investigate.

### 7.4 Subsequent connects (unexpected mismatch, no rebuild)

If the host key changes *and* the codespace was not rebuilt, that's a
potential MITM. We return `HostKeyMismatch` (exit 76) regardless of
`--accept-new-host-key` — the operator must investigate manually.

The decision matrix lives in `src/ssh/host_keys.rs` (`decide` and
`enforce_decision`):

```
                     | stored = None | stored == incoming | stored != incoming, stale | stored != incoming, fresh
                     | (first)        | (match)            | (rebuilt)                 | (suspicious)
---------------------+----------------+--------------------+---------------------------+---------------------------
decision             | StoreNew       | Match              | Rotate{old,new}           | Reject{expected,actual}
enforce (accept=F)   | Ok(None)       | Ok(Some("match"))  | Err(HostKeyMismatch)      | Err(HostKeyMismatch)
enforce (accept=T)   | Ok(None)       | Ok(Some("match"))  | Ok(Some("rotated:..."))   | Err(HostKeyMismatch)
```

Note `Reject` always errors — `--accept-new-host-key` does NOT bypass a
mismatch with no rebuild explanation.

---

## 8. Error model

### 8.1 Closed set of 18 kinds

Every error in `codespacectl` is a `CodespaceError` (see `src/error.rs`).
There are exactly 18 variants, each with:

- `kind(&self) -> &'static str` — stable string identifier (used in the
  JSON envelope's `error.kind` field).
- `retryable(&self) -> bool` — should the caller retry?
- `suggested_action(&self) -> &'static str` — one-line next step for the
  operator/agent.
- `context(&self) -> Option<serde_json::Value>` — structured payload (e.g.
  `expected_fingerprint`/`actual_fingerprint` for `host_key_mismatch`).
- `exit_code(&self) -> i32` — sysexits.h-style process exit code.

The full catalog is in [CLI_REFERENCE.md §4](./CLI_REFERENCE.md#4-error-catalog).

### 8.2 Why a closed set?

A closed set means callers (agents, scripts, CI) can write a `switch` over
`error.kind` without worrying about new kinds sneaking in. New kinds require
a major version bump of the JSON schema (`codespacectl/v2`).

### 8.3 Why `retryable`?

Codespace operations are inherently flaky: network glitches, API rate
limits, codespace start delays. Annotating each error with `retryable: true`
lets agents decide whether to retry with backoff (`codespace_start_timeout`,
`codespace_unreachable`, `command_timeout`, `network_error`) versus
escalate (`auth_failed`, `host_key_mismatch`, `manifest_invalid`).

### 8.4 Conversions

`CodespaceError` has `From` impls for:

- `std::io::Error` → `NetworkError` (or `ManifestInvalid` if a YAML file
  read fails during parsing — handled by the call site).
- `serde_json::Error` → `Internal`
- `serde_yaml::Error` → `ManifestInvalid`
- `reqwest::Error` → `CodespaceUnreachable` (timeouts/connect) or
  `NetworkError` (other)
- `SshError` (from `ssh/exec.rs`) → `Internal` (catch-all; re-classified
  to `CommandTimeout` by `exec/mod.rs::classify_ssh_err` when the message
  contains "timed out")
- `SecretError` (from `secrets/storage.rs`) → `Internal`

This means a low-level error never leaks as a generic `Box<dyn Error>` —
it's always one of the 18 kinds.

---

## 9. Session log

### 9.1 What it is

Every `connect`, `exec`, `raw`, `health`, and `stop` invocation opens an
NDJSON (newline-delimited JSON) log file at
`~/.cache/codespacectl/sessions/<uuid>.ndjson`. The session ID is a
random UUIDv4; the path is returned in the JSON envelope as
`session.log_path`.

### 9.2 Why NDJSON?

- **Append-only**: each entry is a complete JSON object written with a
  trailing newline. No partial writes, no parser state.
- **Streamable**: tools like `jq -c` can process the file line by line.
- **Replayable**: `codespacectl session log --session <id>` reads the file
  back into a `Vec<SessionEntry>` and pretty-prints it.

### 9.3 Entry shape

```json
{
  "timestamp": "2026-01-20T12:34:56.789+00:00",
  "kind": "exec_start",
  "data": { "command": "test", "rendered": "cd /workspaces/... && cargo test", "timeout_secs": 300 }
}
```

`kind` is one of 9 variants (`connect`, `exec_start`, `exec_output`,
`exec_end`, `health_check`, `hook`, `stop`, `warning`, `error`).

### 9.4 Best-effort logging

Session log writes are **best-effort** — if the log file is unwritable, the
underlying exec still succeeds and the result is returned to the caller.
This is enforced by `exec::log_best_effort` which wraps
`session.append(...)` in `let _ = ...`. We don't want a log write failure
to mask a successful command.

### 9.5 Listing

`SessionLog::list_recent(n)` scans the sessions directory, sorts by
modification time (most recent first), and truncates to N. This backs
`codespacectl session log --last N`.

---

## 10. Secret store

### 10.1 Where things live

- **Identity**: `~/.config/codespacectl/identity.age` — a single
  `age::x25519::Identity` in bech32-encoded form, generated lazily on first
  use. `0600` perms on Unix.
- **Secrets**: `~/.cache/codespacectl/secrets/<name>.age` — one
  ASCII-armored age blob per secret, encrypted to the identity's recipient.
  `0600` perms on Unix.

Both paths honor `$XDG_CONFIG_HOME` and `$XDG_CACHE_HOME` via `dirs`.

### 10.2 Encryption details

- `age 0.11.5` with the `armor` feature.
- `Encryptor::with_recipients(iter)` (takes an iterator of `&dyn Recipient`).
- The output is wrapped first in `ArmoredWriter::wrap_output(output,
  Format::AsciiArmor)` (so the on-disk blob is a multi-line text file), then
  in `Encryptor::wrap_output(armored_writer)` (so the bytes themselves are
  encrypted).
- To decrypt, the file is read into bytes, wrapped in
  `ArmoredReader::new(&bytes)`, then passed to
  `Decryptor::new_buffered(reader).decrypt(identities)`.
- The X25519 identity string is `SecretString` (from `age::secrecy`); reading
  the underlying `&str` requires `ExposeSecret::expose_secret()`.

### 10.3 Lifecycle

- **Init** (`SecretStore::init`): ensures both directories exist; on missing
  identity, generates a fresh `age::x25519::Identity::generate()`, writes it
  to disk, sets `0600` perms.
- **Set** (`SecretStore::set(name, value)`): calls `init`, loads the
  identity, derives the recipient, double-wraps the output, writes the
  encrypted blob to `<secrets_dir>/<name>.age` with `0600` perms.
- **Get** (`SecretStore::get(name)`): returns `SecretError::NotFound` if
  the file is missing, otherwise reads, wraps in `ArmoredReader`, decrypts
  via the identity, trims a single trailing newline if present.
- **Exists** / **Delete**: thin wrappers around `Path::exists` and
  `fs::remove_file`.

### 10.4 Why age (and not gpg, not plain `chmod 0600`)?

- **`chmod 0600` alone** is not enough — anyone with read access to the
  home directory (root, backups, etc.) gets the plaintext secret.
- **GPG** requires a running `gpg-agent`, requires the operator to have
  generated a key, and is awkward to automate. The dependency tree would
  be huge.
- **`age`** is a single Rust crate (`age 0.11`), generates the identity
  lazily on first use, and the identity is a single X25519 key — easy to
  back up (copy one file) and easy to destroy (delete one file).

The trade-off: the identity file is the single point of compromise. If it
leaks, all secrets decrypt. We mitigate by `0600` perms and by never
printing the identity to stdout/stderr/JSON output.

---

## 11. Manifest discovery

`manifest::find_manifest(start_dir)` walks up from `start_dir`, checking each
directory for `CODESPACE.yaml` (then `CODESPACE.yml` as a fallback). The
first match wins. If we reach the filesystem root without a match, returns
`ManifestNotFound`.

This mirrors `git`'s `.git/` discovery: run `codespacectl exec test` from
any subdirectory of a project and it'll find the right manifest. The
`--manifest` global flag overrides discovery entirely (useful for testing
manifests that live outside the repo).

When `codespacectl init <path|URL>` is used, the manifest content is
fetched/copied, SHA-256'd, validated, and cached at
`~/.cache/codespacectl/manifests/<sha>.yaml`. The state file records both
the original path/URL and the cached copy.

---

## 12. Reproducibility

### 12.1 Single static binary

The Linux release binary is built against `x86_64-unknown-linux-musl` (and
`aarch64-unknown-linux-musl`) with `panic = abort`, `lto = true`,
`codegen-units = 1`, and `strip = true` in the release profile (see
`Cargo.toml`). This produces a single ~10 MB fully-static binary with no
shared-library dependencies — it runs on any Linux distro including the
slim agent containers AI ops tools run in.

### 12.2 State export/import

`codespacectl state --export` dumps the state file as pretty JSON to
stdout. `codespacectl state --import <path>` replaces the state file with
the JSON at the given path. This is the cross-machine transfer mechanism:

- CI pipelines can `state --export` after `connect` to capture the host key
  fingerprint, then `state --import` on subsequent runs to skip the first-
  connect fingerprint store.
- Operators can copy state from a laptop to a server (or vice versa) without
  re-doing TOFU for every codespace.

### 12.3 Manifest SHA-256 tracking

`state.current_manifest_sha256` and `state.manifests[name].sha256` track
the SHA-256 of the loaded manifest content. Callers can detect manifest
changes across runs (e.g. "the manifest was edited since the last `connect`,
so re-run health checks") by comparing these values.

### 12.4 Token fingerprint, not the token

`state.token_fingerprint` is `sha256(token)[:8]` — never the full token. It
lets callers detect a token swap (different fingerprint) without the token
ever being persisted in plaintext. The token file at
`~/.config/codespacectl/token` does hold the plaintext (needed so we can
forward it to the `gh` subprocess as `GH_TOKEN`), but it's `0600` perms
and `check_token_file_perms` refuses to use it if the mode is too open.

---

## 13. Module-by-module test strategy

Most modules have unit tests that don't require a live codespace or network:

- `state/file.rs`: `state_dir_is_absolute`, `state_round_trip` (uses
  `tempfile::tempdir` + `XDG_CACHE_HOME`).
- `secrets/storage.rs`: init/round-trip/missing/overwrite/exists/delete +
  Unix-only perms check. Uses a static `Mutex` to serialize env-var-touching
  tests (avoids the cost of a `serial_test` dep).
- `manifest/parser.rs`: validates schema rules V1–V8 against crafted YAML
  strings.
- `manifest/templates.rs`: render_template tests for `{{workingDir}}`,
  `{{secret.NAME}}`, and pass-through of unknown placeholders.
- `ssh/transport.rs`: pin-projection compile check, key path determinism,
  key generation perms, key round-trip (no live SSH needed).
- `ssh/host_keys.rs`: `decide` returns the right `HostKeyDecision` for
  first-connect / match / reject / rotate cases (uses crafted
  `CodespaceState`).
- `health/mod.rs`: `build_report` tests for all-pass / some-fail / empty /
  name round-trip / duration round-trip / `checked_at` format.
- `exec/mod.rs`: `ExecOutput` serialization round-trip, `classify_ssh_err`
  re-classifies internal timeouts to `CommandTimeout` and passes through
  other variants.
- `error.rs`: `token_fingerprint` length/determinism/difference tests.
- `github/auth.rs`: same `token_fingerprint` tests (re-tested there).

End-to-end tests (a live codespace + real network + real `gh` binary) are
deferred to the main agent's integration testing in Wave 8 / Phase 12.
