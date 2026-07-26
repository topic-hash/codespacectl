# codespacectl — CLI Reference

This document is the **authoritative** reference for every `codespacectl`
subcommand, the JSON envelope schema, the error catalog, the state file
format, and the environment variables that influence behavior.

- **Binary**: `codespacectl` (single static Rust binary, ~10 MB)
- **Version**: matches `Cargo.toml` (`0.1.0` at time of writing)
- **Stable output schema**: `codespacectl/v1`

If the implementation and this document disagree, **this document is wrong**
(file an issue).

---

## Table of contents

1. [Global flags](#1-global-flags)
2. [Subcommand reference](#2-subcommand-reference)
   - [init](#init)
   - [discover](#discover)
   - [switch](#switch)
   - [connect](#connect)
   - [health](#health)
   - [exec](#exec)
   - [raw](#raw)
   - [stop](#stop)
   - [state](#state)
   - [session log](#session-log)
   - [doctor](#doctor)
   - [token set | get | clear](#token-set--get--clear)
3. [JSON envelope schema](#3-json-envelope-schema)
4. [Error catalog](#4-error-catalog)
5. [State file format](#5-state-file-format)
6. [Environment variables](#6-environment-variables)
7. [Exit code table](#7-exit-code-table)

---

## 1. Global flags

These flags work on **every** subcommand. They are declared in
`src/cli/args.rs` and parsed by `clap`.

| Flag             | Type      | Default | Description                                                |
|------------------|-----------|---------|------------------------------------------------------------|
| `--json`         | boolean   | `false` | Output a stable JSON envelope instead of human text.       |
| `-v` / `--verbose` | count  | `0`     | Increase log verbosity (`-v`, `-vv`, `-vvv`).             |
| `--manifest <p>` | string   | auto    | Path/URL to a `CODESPACE.yaml` (auto-discovered if unset). |
| `-h` / `--help`  | flag      | —       | Show help.                                                 |
| `-V` / `--version` | flag    | —       | Print version.                                              |

---

## 2. Subcommand reference

For each subcommand this section lists:

- **Description**: one-line summary
- **Usage**: canonical invocation
- **Flags**: full table (type, default, description)
- **JSON `result` schema**: the object placed at `result` in the envelope
- **Example**: one text-mode and one JSON-mode invocation
- **Exit codes**: which codes this subcommand returns
- **Common errors**: which `error.kind` values this subcommand may emit

### `init`

**Description**: Register a manifest by path or URL.

**Usage**: `codespacectl init <path|URL>`

**Flags**:

| Flag  | Type   | Required | Default | Description                                       |
|-------|--------|----------|---------|---------------------------------------------------|
| `path`| positional | yes | — | Local file path or `http(s)://` URL to a `CODESPACE.yaml`. |

**JSON `result` schema**:

```json
{
  "name": "data-migrata",
  "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "cached_path": "/home/user/.cache/codespacectl/manifests/9f86d081....yaml",
  "manifest_count": 2
}
```

**Example (text)**:

```bash
$ codespacectl init ./CODESPACE.yaml
Registered manifest 'data-migrata' from ./CODESPACE.yaml
  SHA-256:        9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
  Cached at:      /home/user/.cache/codespacectl/manifests/9f86d081....yaml
  Total registered: 1
```

**Example (JSON)**:

```bash
$ codespacectl --json init ./CODESPACE.yaml
{
  "schema": "codespacectl/v1",
  "ok": true,
  "result": { "name": "data-migrata", "sha256": "...", "cached_path": "...", "manifest_count": 1 },
  "error": null,
  "warnings": [],
  "session": null
}
```

**Exit codes**: `0` on success; `65` if the manifest fails validation;
`75` on network error fetching the URL.

**Common errors**: `manifest_not_found`, `manifest_invalid`,
`manifest_version_unsupported`, `network_error`.

---

### `discover`

**Description**: List codespaces for the authenticated user, optionally
filtered by repo or state.

**Usage**: `codespacectl discover [--repo <slug>] [--state <state>]`

**Flags**:

| Flag       | Type   | Required | Default | Description                                                            |
|------------|--------|----------|---------|------------------------------------------------------------------------|
| `--repo`   | string | no       | —       | Filter by repository slug substring (e.g. `topic-hash/DataMigrata`).  |
| `--state`  | string | no       | —       | Filter by codespace state (e.g. `Available`, `Shutdown`).             |

**JSON `result` schema** (an array):

```json
[
  {
    "index": 1,
    "name": "symmetrical-tribble-pjvp5rjg5w5v299jq",
    "display_name": "DataMigrata",
    "state": "Available",
    "repository": "topic-hash/DataMigrata",
    "created_at": "2026-01-15T00:00:00Z",
    "last_used_at": "2026-01-20T12:34:56Z",
    "is_current": true
  }
]
```

**Example (text)**:

```bash
$ codespacectl discover
#    NAME                                        STATE          REPO                             CREATED
------------------------------------------------------------------------------------------------------------------------
*  1  symmetrical-tribble-pjvp5rjg5w5v299jq       Available     topic-hash/DataMigrata            2026-01-15T00:00:00Z
   2  psychic-space-fishstick-gxrwv4rprvrcwjwv    Shutdown      topic-hash/three-pillars-voip     2026-01-10T00:00:00Z

(*) = current codespace
Switch with: codespacectl switch --index <N>
```

**Example (JSON)**: `codespacectl --json discover --repo DataMigrata`

**Exit codes**: `0` on success (including empty results); `70` on auth failure.

**Common errors**: `token_missing`, `auth_failed`, `token_revoked`,
`token_invalid_scope`, `codespace_unreachable`, `network_error`.

---

### `switch`

**Description**: Change the current codespace pointer in state. **Does not
connect** — call `connect` after.

**Usage**: `codespacectl switch [--codespace <name> | --index <N>]`

**Flags**:

| Flag          | Type    | Required | Default | Description                                                |
|---------------|---------|----------|---------|------------------------------------------------------------|
| `--codespace` | string  | no       | —       | Full or partial codespace name (first match wins).        |
| `--index`     | integer | no       | —       | 1-indexed position from `discover` output.                |

Without either flag, in a TTY this opens an interactive picker. In a
non-TTY (or with `--json`), it prints the discovery list and exits 0.

**JSON `result` schema**:

```json
{
  "previous_codespace": "symmetrical-tribble-pjvp5rjg5w5v299jq",
  "current_codespace": "psychic-space-fishstick-gxrwv4rprvrcwjwv",
  "state": "Shutdown",
  "repository": "topic-hash/three-pillars-voip",
  "note": "Run `codespacectl connect` to establish SSH session."
}
```

**Example (text)**:

```bash
$ codespacectl switch --index 2
Switched codespace: symmetrical-tribble-pjvp5rjg5w5v299jq -> psychic-space-fishstick-gxrwv4rprvrcwjwv
  state:    Shutdown
  repo:     topic-hash/three-pillars-voip

Run `codespacectl connect` to establish SSH session.
```

**Example (JSON)**: `codespacectl --json switch --codespace psychic-space`

**Exit codes**: `0` on success; `70` if the codespace is not found.

**Common errors**: `codespace_not_found`, `auth_failed`, `token_missing`,
`codespace_unreachable`.

---

### `connect`

**Description**: Bring a codespace to `Available`, establish SSH over
`gh cs ssh --stdio`, perform TOFU host-key verification, run `postStart`
hooks, and run health checks.

**Usage**: `codespacectl connect --codespace <name> [options]`

**Flags**:

| Flag                      | Type    | Required | Default | Description                                                          |
|---------------------------|---------|----------|---------|--------------------------------------------------------------------|
| `--codespace`             | string  | yes      | —       | Codespace name (full or partial — first match wins).               |
| `--accept-new-host-key`   | boolean | no       | `false` | Accept a rotated host key (after rebuild). First connect always stores. |
| `--skip-health`           | boolean | no       | `false` | Skip the health check pass.                                        |
| `--skip-hooks`            | boolean | no       | `false` | Skip `postStart` hooks.                                            |
| `--timeout`               | integer | no       | `180`   | Seconds to wait for the codespace to reach `Available`.            |

**JSON `result` schema**:

```json
{
  "codespace": "symmetrical-tribble-pjvp5rjg5w5v299jq",
  "state": "Available",
  "manifest": "/path/to/CODESPACE.yaml",
  "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "host_key_fingerprint": "SHA256:abc123...",
  "host_key_decision": "match",
  "hooks_ran": 2,
  "health": {
    "overall": "green",
    "checks": [
      { "name": "docker", "passed": true, "exit_code": 0, "stdout": "...", "stderr": "", "duration_secs": 0.42 }
    ],
    "checked_at": "2026-01-20T12:34:56.789+00:00"
  }
}
```

`host_key_decision` is one of: `store_new`, `match`, `rotate`, `reject`, `unknown`.

**Example (text)**:

```bash
$ codespacectl connect --codespace symmetrical-tribble
Connected to codespace 'symmetrical-tribble-pjvp5rjg5w5v299jq'
  State:             Available
  Manifest:          /workspaces/DataMigrata/CODESPACE.yaml
  SHA-256:           9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
  Host key:          SHA256:abc123... (match)
  postStart hooks:   2
  Health:            green (3 checks at 2026-01-20T12:34:56.789+00:00)
  Session log:       /home/user/.cache/codespacectl/sessions/550e8400-....ndjson
```

**Example (JSON)**: `codespacectl --json connect --codespace symmetrical-tribble --skip-health`

**Exit codes**: `0` on success; `70` on auth/host-key-mismatch/health-check-failed;
`75` on start-timeout/network; `76` on host-key reject (possible MITM).

**Common errors**: `token_missing`, `auth_failed`, `codespace_not_found`,
`codespace_start_timeout`, `codespace_unreachable`, `host_key_mismatch`,
`command_failed` (from `postStart`), `command_timeout` (from `postStart`),
`health_check_failed`, `binary_missing`.

---

### `health`

**Description**: Run the manifest's health checks against a connected
codespace and print the report.

**Usage**: `codespacectl health [--codespace <name>]`

**Flags**:

| Flag          | Type   | Required | Default | Description                                              |
|---------------|--------|----------|---------|----------------------------------------------------------|
| `--codespace` | string | no       | state's `current_codespace` | Override the target codespace.           |

**JSON `result` schema**:

```json
{
  "overall": "green",
  "checks": [
    {
      "name": "docker",
      "passed": true,
      "exit_code": 0,
      "stdout": "...",
      "stderr": "",
      "duration_secs": 0.42
    }
  ],
  "checked_at": "2026-01-20T12:34:56.789+00:00"
}
```

`overall` is `"green"` or `"red"`.

**Example (text)**:

```bash
$ codespacectl health
Health: green (codespace 'symmetrical-tribble-pjvp5rjg5w5v299jq', 3 checks at 2026-01-20T12:34:56.789+00:00)
  OK    docker                         exit=0  0.42s
  OK    cargo                          exit=0  0.18s
  OK    sql-server                     exit=0  0.93s
Session log: /home/user/.cache/codespacectl/sessions/550e8400-....ndjson
```

**Example (JSON)**: `codespacectl --json health`

**Exit codes**: `0` if `overall == green`; `1` if `overall == red` (the
process exit code signals red/green for shell scripts); `70` on auth/transport
errors.

**Common errors**: `token_missing`, `auth_failed`, `codespace_unreachable`,
`network_error`, `host_key_mismatch`, `internal_error` (codespace not in
state — run `connect` first).

---

### `exec`

**Description**: Look up a manifest-declared command by name, run a health
gate (unless `--force`), open an SSH session, execute the command, and
propagate the remote exit code as the process exit code.

**Usage**: `codespacectl exec <command-name> [--codespace <name>] [--force] [--timeout <secs>]`

**Flags**:

| Flag          | Type    | Required | Default | Description                                                |
|---------------|---------|----------|---------|------------------------------------------------------------|
| `command-name`| positional | yes   | —       | Key into the manifest's `commands` map.                    |
| `--codespace` | string  | no       | state's `current_codespace` | Override the target codespace.           |
| `--force`     | boolean | no       | `false` | Skip the health gate (run even if health is red).          |
| `--timeout`   | integer | no       | manifest's `timeoutSecs` | Override the per-command timeout.          |

**JSON `result` schema** (`ExecOutput`):

```json
{
  "command_name": "test",
  "stdout": "running 42 tests...\nall passed\n",
  "stderr": "warning: deprecated flag\n",
  "exit_code": 0,
  "duration_secs": 12.4,
  "session_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

A non-zero remote exit code is **not** an error — it's returned as
`Ok(ExecOutput)` with `exit_code` set, and `codespacectl` exits with that
exit code. Only transport failures and timeouts propagate as `Err`.

**Example (text)**:

```bash
$ codespacectl exec test
running 42 tests...
all passed
[test exit 0 in 12.40s, session 550e8400-e29b-41d4-a716-446655440000]
```

**Example (JSON)**: `codespacectl --json exec build --timeout 600`

**Exit codes**: `0` if the remote command exits 0; the **remote exit code**
if non-zero (1–255); `70` on auth/transport/health-gate; `75` on timeout.

**Common errors**: `token_missing`, `auth_failed`, `manifest_invalid`
(unknown command name), `health_check_failed` (health gate), `command_timeout`,
`codespace_unreachable`, `host_key_mismatch`, `binary_missing`.

---

### `raw`

**Description**: Execute an ad-hoc shell command on the codespace. No
manifest lookup, no template substitution, no health gate.

**Usage**: `codespacectl raw <command> [--codespace <name>] [--timeout <secs>]`

**Flags**:

| Flag          | Type    | Required | Default | Description                                                |
|---------------|---------|----------|---------|------------------------------------------------------------|
| `command`     | positional | yes   | —       | Shell command to execute verbatim.                         |
| `--codespace` | string  | no       | state's `current_codespace` | Override the target codespace.           |
| `--timeout`   | integer | no       | `300`   | Per-command SSH exec timeout.                              |

**JSON `result` schema** (same `ExecOutput` as `exec`, with `command_name = "raw"`):

```json
{
  "command_name": "raw",
  "stdout": "...",
  "stderr": "",
  "exit_code": 0,
  "duration_secs": 0.05,
  "session_id": "..."
}
```

**Example (text)**:

```bash
$ codespacectl raw 'uname -a'
Linux codespace-xyz 5.15.0-1019-azure ... x86_64 GNU/Linux
[raw exit 0 in 0.05s, session 550e8400-e29b-41d4-a716-446655440000]
```

**Example (JSON)**: `codespacectl --json raw 'df -h /workspaces'`

**Exit codes**: `0` if the remote command exits 0; the **remote exit code**
if non-zero; `70`/`75` on transport/timeout.

**Common errors**: `token_missing`, `auth_failed`, `command_timeout`,
`codespace_unreachable`, `host_key_mismatch`, `binary_missing`,
`internal_error` (no current codespace — run `connect` first).

---

### `stop`

**Description**: Run `preStop` hooks (unless `--skip-hooks`), call the GitHub
Codespaces API to stop the codespace, update state.

**Usage**: `codespacectl stop [--codespace <name>] [--skip-hooks]`

**Flags**:

| Flag          | Type    | Required | Default | Description                                                |
|---------------|---------|----------|---------|------------------------------------------------------------|
| `--codespace` | string  | no       | state's `current_codespace` | Override the target codespace.           |
| `--skip-hooks`| boolean | no       | `false` | Skip `preStop` hooks.                                      |

**JSON `result` schema**:

```json
{
  "codespace": "symmetrical-tribble-pjvp5rjg5w5v299jq",
  "state": "Shutdown",
  "hooks_ran": 1
}
```

**Example (text)**:

```bash
$ codespacectl stop
Stopped codespace 'symmetrical-tribble-pjvp5rjg5w5v299jq' (preStop hooks run: 1)
```

**Example (JSON)**: `codespacectl --json stop --skip-hooks`

**Exit codes**: `0` on success; `70` if `preStop` hook fails or stop API errors.

**Common errors**: `token_missing`, `auth_failed`, `codespace_not_found`,
`command_failed` (from `preStop`), `command_timeout` (from `preStop`),
`codespace_unreachable`, `network_error`, `binary_missing`.

---

### `state`

**Description**: Inspect or transfer the local state file. Without flags:
print the state file path and a brief summary.

**Usage**: `codespacectl state [--export | --import <path>]`

**Flags**:

| Flag       | Type    | Required | Default | Description                                              |
|------------|---------|----------|---------|----------------------------------------------------------|
| `--export` | boolean | no       | `false` | Dump the state file as pretty-printed JSON to stdout.    |
| `--import` | string  | no       | —       | Replace the state file with the JSON at the given path.   |

**JSON `result` schema (no flags)**:

```json
{
  "state_file": "/home/user/.cache/codespacectl/state.json",
  "current_codespace": "symmetrical-tribble-pjvp5rjg5w5v299jq",
  "current_manifest": "/path/to/CODESPACE.yaml",
  "current_manifest_sha256": "9f86d081...",
  "codespaces_tracked": 2,
  "manifests_registered": 1
}
```

With `--export`: `result` is the state file's JSON content directly.

With `--import <path>`:

```json
{ "imported": true, "path": "/tmp/state-export.json" }
```

**Example (text)**:

```bash
$ codespacectl state
State file: /home/user/.cache/codespacectl/state.json
Current codespace:    symmetrical-tribble-pjvp5rjg5w5v299jq
Current manifest:     /path/to/CODESPACE.yaml
Manifest SHA-256:     9f86d081...
Codespaces tracked:   2
Manifests registered: 1
```

**Example (JSON)**: `codespacectl --json state --export > backup.json`

**Exit codes**: `0` on success; `70` on parse/IO error.

**Common errors**: `internal_error` (state file corrupted or unreadable),
`network_error` (file IO mapped from `std::io::Error`).

---

### `session log`

**Description**: Inspect session logs. Without `--session`: list the N most
recent session IDs (most recent first). With `--session <id>`: dump every
entry in that session's NDJSON log.

**Usage**:
- `codespacectl session log [--last <N>]`
- `codespacectl session log --session <id>`

**Flags**:

| Flag        | Type    | Required | Default | Description                                                |
|-------------|---------|----------|---------|------------------------------------------------------------|
| `--last`    | integer | no       | `5`     | How many recent sessions to list.                          |
| `--session` | string  | no       | —       | Show all entries of this session ID (UUID).                 |

**JSON `result` schema (list mode)**:

```json
{
  "sessions": [
    { "id": "550e8400-e29b-41d4-a716-446655440000", "modified_at": "..." }
  ],
  "count": 1
}
```

**JSON `result` schema (show mode)**:

```json
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "entries": [
    {
      "timestamp": "2026-01-20T12:34:56.789+00:00",
      "kind": "connect",
      "data": { "codespace": "...", "manifest": "..." }
    },
    {
      "timestamp": "2026-01-20T12:35:00.000+00:00",
      "kind": "exec_start",
      "data": { "command": "test", "rendered": "...", "timeout_secs": 300 }
    }
  ],
  "count": 2
}
```

`kind` is one of: `connect`, `exec_start`, `exec_output`, `exec_end`,
`health_check`, `hook`, `stop`, `warning`, `error`.

**Example (text)**:

```bash
$ codespacectl session log --last 3
Recent 3 session(s):
  550e8400-e29b-41d4-a716-446655440000  (modified ...)
  11111111-2222-3333-4444-555555555555  (modified ...)
  99999999-aaaa-bbbb-cccc-dddddddddddd  (modified ...)

$ codespacectl session log --session 550e8400-e29b-41d4-a716-446655440000
Session 550e8400-... (12 entries):
[2026-01-20T12:34:56.789+00:00] Connect  {"codespace":"...","manifest":"..."}
[2026-01-20T12:35:00.000+00:00] ExecStart  {"command":"test","rendered":"...","timeout_secs":300}
...
```

**Example (JSON)**: `codespacectl --json session log --session 550e8400-...`

**Exit codes**: `0` on success; `70` on parse/IO error.

**Common errors**: `internal_error` (NDJSON file unreadable or malformed).

---

### `doctor`

**Description**: Run environment checks (python3, rustc, token, state file,
registered manifests, `gh` binary, network reachability to
`api.github.com`) and report results. Exits 0 if all checks pass.

**Usage**: `codespacectl doctor`

**Flags**: none.

**JSON `result` schema**:

```json
{
  "all_ok": true,
  "checks": [
    { "name": "python3",     "ok": true,  "detail": "Python 3.11.6" },
    { "name": "rustc",       "ok": true,  "detail": "rustc 1.97.1" },
    { "name": "token",       "ok": true,  "detail": "token resolved (env var or token file)" },
    { "name": "token_file",  "ok": true,  "detail": "/home/user/.config/codespacectl/token (exists: true)" },
    { "name": "state_file",  "ok": true,  "detail": "/home/user/.cache/codespacectl/state.json" },
    { "name": "manifests_registered", "ok": true, "detail": "1 manifest(s) registered" },
    { "name": "gh_binary",   "ok": true,  "detail": "/workspaces/.../tools/bin/gh" },
    { "name": "network",     "ok": true,  "detail": "https://api.github.com -> HTTP 200" }
  ]
}
```

**Example (text)**:

```bash
$ codespacectl doctor
codespacectl doctor
---------------------------
OK   python3                      Python 3.11.6
OK   rustc                        rustc 1.97.1
OK   token                        token resolved (env var or token file)
OK   token_file                   /home/user/.config/codespacectl/token (exists: true)
OK   state_file                   /home/user/.cache/codespacectl/state.json
OK   manifests_registered         1 manifest(s) registered
OK   gh_binary                    /workspaces/.../tools/bin/gh
OK   network                       https://api.github.com -> HTTP 200
---------------------------
All checks passed.
```

**Example (JSON)**: `codespacectl --json doctor`

**Exit codes**: `0` if `all_ok`; `1` if any check failed (note: this is **1**,
not in the sysexits table — `doctor` is a status-reporting command, not a
protocol operation).

**Common errors**: none (it never returns `Err` — every failure is folded
into the `checks` array).

---

### `token set | get | clear`

**Description**: Manage the persisted GitHub PAT. The token is stored at
`~/.config/codespacectl/token` with `0600` perms on Unix. The env var
`CODESPACECTL_TOKEN` takes precedence over the file.

#### `token set`

**Usage**: `codespacectl token set`

Reads the token from **stdin** (no echo suppression — redirect from a file
to keep it out of shell history). Prints a warning about echo being on.

**Flags**: none.

**JSON `result` schema**:

```json
{ "saved": true, "path": "/home/user/.config/codespacectl/token" }
```

**Example (text)**:

```bash
$ codespacectl token set < /path/to/token
warning: stdin echo is on. To suppress, redirect from a file: `codespacectl token set < /path/to/token`.
Token saved to /home/user/.config/codespacectl/token
```

**Example (JSON)**: `codespacectl --json token set < /path/to/token`

**Exit codes**: `0` on success; `70` if stdin is empty or IO fails.

**Common errors**: `internal_error` (empty stdin, IO failure).

#### `token get`

**Usage**: `codespacectl token get`

Prints the token file path. **Never** prints the token itself.

**Flags**: none.

**JSON `result` schema**:

```json
{ "path": "/home/user/.config/codespacectl/token", "exists": true }
```

**Example (text)**:

```bash
$ codespacectl token get
Token file: /home/user/.config/codespacectl/token
(token is stored; contents not displayed for security)
```

**Example (JSON)**: `codespacectl --json token get`

**Exit codes**: `0` (always — even if the file is missing).

**Common errors**: none.

#### `token clear`

**Usage**: `codespacectl token clear`

Deletes the token file (no-op if absent).

**Flags**: none.

**JSON `result` schema**:

```json
{ "cleared": true, "path": "/home/user/.config/codespacectl/token" }
```

`cleared` is `true` if a file was removed, `false` if it was already absent.

**Example (text)**:

```bash
$ codespacectl token clear
Token file removed: /home/user/.config/codespacectl/token
```

**Example (JSON)**: `codespacectl --json token clear`

**Exit codes**: `0` on success; `70` on IO failure.

**Common errors**: `internal_error` (IO failure).

---

## 3. JSON envelope schema

Every command, when invoked with `--json`, emits exactly one JSON object on
stdout (no trailing newline-free output, no interleaved logging). The
schema identifier is `codespacectl/v1`.

```json
{
  "schema": "codespacectl/v1",
  "ok": true,
  "result": { /* command-specific */ },
  "error": null,
  "warnings": [],
  "session": null
}
```

| Field      | Type                  | Always present | Description                                                       |
|------------|-----------------------|----------------|-------------------------------------------------------------------|
| `schema`   | string                | yes            | Always `"codespacectl/v1"`.                                       |
| `ok`       | boolean               | yes            | `true` on success, `false` on error.                            |
| `result`   | object or `null`      | yes            | Command-specific payload. `null` when `ok == false`.              |
| `error`    | `ErrorEnvelope` or `null` | yes        | Present iff `ok == false`. See §4.                                |
| `warnings` | array of strings      | yes            | Non-fatal warnings (currently always `[]`).                       |
| `session`  | `SessionRef` or `null` | yes            | Present iff this command opened a session log (most commands).    |

### 3.1 `SessionRef`

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "log_path": "/home/user/.cache/codespacectl/sessions/550e8400-....ndjson"
}
```

### 3.2 Example: error envelope

```json
{
  "schema": "codespacectl/v1",
  "ok": false,
  "result": null,
  "error": {
    "kind": "health_check_failed",
    "message": "health check failed: docker",
    "retryable": false,
    "suggested_action": "Run `codespacectl doctor` on the codespace, or `codespacectl connect --force`",
    "context": { "check": "docker", "exit_code": 1, "stderr": "Cannot connect to the Docker daemon..." }
  },
  "warnings": [],
  "session": null
}
```

---

## 4. Error catalog

A closed set of 18 error kinds. Every `error.kind` value comes from this
table. The implementation lives in `src/error.rs`.

| #  | kind                            | retryable | exit | description                                  | suggested_action                                                                       |
|----|---------------------------------|-----------|------|----------------------------------------------|----------------------------------------------------------------------------------------|
| 1  | `binary_missing`                | false     | 70   | gh binary not found                          | Install the binary or set `CODESPACECTL_GH_BIN` env var                                 |
| 2  | `binary_hash_mismatch`          | false     | 70   | gh binary SHA-256 mismatch                   | Re-download the binary from a trusted source                                           |
| 3  | `auth_failed`                   | false     | 70   | 401 from GitHub API                          | Regenerate the GitHub PAT and re-export `CODESPACECTL_TOKEN`                            |
| 4  | `token_revoked`                 | false     | 70   | 401 + token invalid                          | Regenerate the GitHub PAT and re-export `CODESPACECTL_TOKEN`                            |
| 5  | `token_invalid_scope`           | false     | 70   | 403 missing scope                            | Regenerate PAT with `codespace` (and `repo` if pushing) scope                          |
| 6  | `token_missing`                 | false     | 65   | No env var or token file                    | Set `CODESPACECTL_TOKEN` env var or run `codespacectl token set`                       |
| 7  | `codespace_not_found`           | false     | 70   | 404 from GitHub                             | Check the codespace name, or run `codespacectl discover`                              |
| 8  | `codespace_start_timeout`       | true      | 75   | Codespace didn't reach `Available`          | Retry with `--timeout 600`, or check `status.github.com`                              |
| 9  | `codespace_unreachable`         | true      | 75   | Network error to GitHub                     | Check network, or retry with `--timeout 600`                                          |
| 10 | `health_check_failed`           | false     | 70   | Manifest health check returned non-zero     | Run `codespacectl doctor`, or `codespacectl connect --force`                           |
| 11 | `command_timeout`               | true      | 75   | exec exceeded `timeoutSecs`                 | Increase `timeoutSecs` in manifest, or run command in chunks                          |
| 12 | `command_failed`                | false     | 70   | Hook exec returned non-zero                 | Inspect the command output in the session log                                         |
| 13 | `host_key_mismatch`             | false     | 76   | SSH host key changed unexpectedly           | If codespace was rebuilt, run `codespacectl connect --accept-new-host-key`             |
| 14 | `manifest_invalid`              | false     | 65   | `CODESPACE.yaml` schema violation          | Validate `CODESPACE.yaml` against [MANIFEST_SPEC.md](./MANIFEST_SPEC.md)               |
| 15 | `manifest_version_unsupported`  | false     | 65   | `apiVersion` not `v1`                       | Upgrade `codespacectl`, or use `apiVersion: v1`                                        |
| 16 | `manifest_not_found`            | false     | 65   | No `CODESPACE.yaml` at path                 | Provide path via `--manifest`, or run `codespacectl init`                              |
| 17 | `network_error`                 | true      | 75   | Generic network failure                     | Check network and retry                                                                |
| 18 | `internal_error`                | false     | 70   | Unexpected (bug)                           | Report a bug at https://github.com/topic-hash/codespacectl/issues                      |

### 4.1 `ErrorEnvelope` JSON shape

```json
{
  "kind": "<one of the 18>",
  "message": "<human-readable>",
  "retryable": <bool>,
  "suggested_action": "<one-line>",
  "context": <object|null>
}
```

`context` is set only for errors that carry structured data:

- `binary_hash_mismatch` → `{ "expected_sha256": "...", "actual_sha256": "..." }`
- `token_invalid_scope` → `{ "missing_scope": "codespace" }`
- `codespace_start_timeout` → `{ "elapsed_secs": 180 }`
- `health_check_failed` → `{ "check": "docker", "exit_code": 1, "stderr": "..." }`
- `command_failed` → `{ "exit_code": 2, "stderr": "..." }`
- `host_key_mismatch` → `{ "expected_fingerprint": "SHA256:...", "actual_fingerprint": "SHA256:..." }`

All other kinds → `context: null`.

---

## 5. State file format

The state file lives at `~/.cache/codespacectl/state.json` (Linux),
`~/Library/Caches/codespacectl/state.json` (macOS), or
`%LOCALAPPDATA%\codespacectl\state.json` (Windows). It is written atomically
(write-to-temp-then-rename) with `0600` permissions on Unix.

```json
{
  "version": 1,
  "current_codespace": "symmetrical-tribble-pjvp5rjg5w5v299jq",
  "current_manifest": "/workspaces/DataMigrata/CODESPACE.yaml",
  "current_manifest_sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "codespaces": {
    "symmetrical-tribble-pjvp5rjg5w5v299jq": {
      "last_known_state": "Available",
      "last_checked_at": "2026-01-20T12:34:56.789+00:00",
      "created_at": "2026-01-15T00:00:00Z",
      "host_key_fingerprint": "SHA256:abc123...",
      "host_key_stored_at": "2026-01-20T12:34:56.789+00:00",
      "last_health_status": "green",
      "last_health_checked_at": "2026-01-20T12:34:56.789+00:00"
    }
  },
  "manifests": {
    "data-migrata": {
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
      "last_validated_at": "2026-01-20T12:34:56.789+00:00"
    }
  },
  "token_fingerprint": "9f86d081"
}
```

### 5.1 Top-level fields

| Field                       | Type                          | Description                                                          |
|-----------------------------|-------------------------------|----------------------------------------------------------------------|
| `version`                   | integer                       | State schema version (currently `1`).                              |
| `current_codespace`         | string or null                | Last `connect`ed codespace (used by `exec`/`raw`/`health` without `--codespace`). |
| `current_manifest`         | string or null                | Last loaded manifest path.                                          |
| `current_manifest_sha256`   | string or null                | SHA-256 of the last loaded manifest.                               |
| `codespaces`                | `Map<name, CodespaceState>`   | Per-codespace state, keyed by full codespace name.                  |
| `manifests`                 | `Map<name, ManifestState>`    | Per-manifest state, keyed by manifest `metadata.name`.             |
| `token_fingerprint`         | string or null                | `sha256(token)[:8]` — for revocation detection. **Never** the full token. |

### 5.2 `CodespaceState`

| Field                  | Type           | Description                                                                |
|------------------------|----------------|----------------------------------------------------------------------------|
| `last_known_state`     | string or null | `"Available"`, `"Shutdown"`, `"Starting"`, `"ShuttingDown"`, etc.         |
| `last_checked_at`      | string or null | ISO 8601 of last state check.                                             |
| `created_at`           | string or null | Codespace creation time (from GitHub API). Used for host-key rotation.    |
| `host_key_fingerprint`| string or null | `"SHA256:..."` (OpenSSH format).                                          |
| `host_key_stored_at`   | string or null | When the host key was first stored.                                        |
| `last_health_status`   | string or null | `"green"` or `"red"`. Null = never checked.                               |
| `last_health_checked_at`| string or null | ISO 8601 of last health check.                                            |

### 5.3 `ManifestState`

| Field               | Type           | Description                              |
|---------------------|----------------|------------------------------------------|
| `sha256`            | string or null | SHA-256 of the manifest content.         |
| `last_validated_at` | string or null | ISO 8601 of last validation.             |

### 5.4 Atomicity and reproducibility

- Writes go through `save_state` which writes a `.tmp` file, then renames
  it over the target. On Unix the file is then `chmod 0600`.
- Concurrent writers (two `codespacectl` processes) race via load → modify →
  save; last-writer-wins. There is no file locking today — file an issue if
  you hit this.
- `codespacectl state --export` / `--import` is the cross-machine transfer
  mechanism. The export is a plain JSON dump that another host can `--import`
  to clone state (e.g. for CI baselines).

---

## 6. Environment variables

| Name                    | Default         | Description                                                                       |
|-------------------------|-----------------|-----------------------------------------------------------------------------------|
| `CODESPACECTL_TOKEN`    | (unset)         | GitHub PAT (takes precedence over the token file).                                |
| `CODESPACECTL_GH_BIN`   | (unset)         | Path to the `gh` binary (skips PATH + `tools/bin/gh` lookup).                     |
| `CODESPACECTL_WORKDIR`  | (unset)         | Override the codespace working directory for this invocation (rarely needed).     |
| `GH_TOKEN`              | (unset)         | Passed through to the `gh cs ssh --stdio` subprocess as `GH_TOKEN` (fallback if `CODESPACECTL_TOKEN` is unset). |
| `XDG_CACHE_HOME`        | `~/.cache`      | Moves the `codespacectl/` cache (state, sessions, secrets, ssh keys).             |
| `XDG_CONFIG_HOME`       | `~/.config`     | Moves the `codespacectl/` config (token, age identity).                          |
| `RUST_LOG`              | (unset)         | Tracing filter (e.g. `info,codespacectl=debug`).                                 |
| `RUST_BACKTRACE`        | (unset)         | `1` or `full` enables backtraces on panic.                                        |

### 6.1 Precedence rules

- **Token resolution** (`resolve_token`): `CODESPACECTL_TOKEN` env var →
  token file → `CodespaceError::TokenMissing`.
- **`gh` binary resolution** (`resolve_gh_bin`): `CODESPACECTL_GH_BIN` env
  var → `<manifest-dir>/tools/bin/gh` → `gh` on `PATH` →
  `CodespaceError::BinaryMissing`.
- **Codespace name resolution** (`resolve_codespace_name`): `--codespace`
  CLI flag → `state.current_codespace` → `CodespaceError::Internal`.

---

## 7. Exit code table

`codespacectl` uses `sysexits.h`-style codes (so scripts can branch on
them) with a few additions for SSH-specific failures.

| Exit | Meaning                                            | When                                            |
|------|----------------------------------------------------|-------------------------------------------------|
| 0    | Success                                            | Every command's happy path.                     |
| 1    | Soft failure (status-only)                          | `doctor` (any check failed), `health` (red).    |
| 2–64 | Reserved / not used by `codespacectl`               | —                                               |
| 65   | Config error (sysexits `EX_DATAERR`/`EX_USAGE`)     | `token_missing`, `manifest_*` errors.           |
| 70   | Internal / non-retryable failure (`EX_SOFTWARE`)    | Auth, host keys, command failures, internal bug. |
| 75   | Temporary failure, retry (`EX_TEMPFAIL`)            | Start timeouts, network errors, exec timeouts.  |
| 76   | Protocol error (`EX_PROTOCOL`)                      | `host_key_mismatch` (possible MITM).            |
| 1–255| Remote command exit code                           | `exec`/`raw` propagate the SSH exec's exit code. |

The mapping is implemented in `CodespaceError::exit_code()` in `src/error.rs`.
