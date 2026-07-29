# codespacectl

> **Manifest-driven CLI for agent-driven GitHub Codespace operations.**
>
> Single Rust binary. No system SSH. No daemon. Token from env var.
> Codespace identity passed by name. State persisted on disk.

## What It Is

`codespacectl` is a CLI tool that lets AI agents (or shell scripts) drive
GitHub Codespaces reliably and reproducibly. It replaces ad-hoc Python scripts
and 132-line agent prompts with:

- A **declarative manifest** (`CODESPACE.yaml`) at any repo root
- A **stable JSON API** for callers to consume
- A **state file** for idempotency across runs
- **Structured error model** with `kind`, `retryable`, `suggested_action`
- **No system packages** required (no ssh, no ssh-keygen, no sudo)

---

## How to Use codespacectl

This section is the complete step-by-step guide. It is written for both humans
and AI agents — every command shown here is copy-paste ready. For the full
flag reference and error catalog, see [CLI Reference](docs/CLI_REFERENCE.md).

### Prerequisites

| Requirement | Details |
|---|---|
| **GitHub PAT** | Fine-grained personal access token with `codespace` scope. Add `repo` scope if you need to push commits. Generate at `https://github.com/settings/tokens?type=beta`. |
| **OS / arch** | Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows (x86_64). Static binary, no system dependencies. |

### Step 1 — Install

The bootstrap script resolves a binary through a tiered lookup — bundled
pre-compiled binaries first (zero network), then local install, then cache,
then GitHub Releases download with SHA-256 verification. No sudo.

```bash
# From the internet (one-liner)
curl -fsSL https://github.com/topic-hash/codespacectl/raw/main/scripts/bootstrap.sh | bash

# From a local clone (uses bundled binary, zero network, ~40ms)
bash scripts/bootstrap.sh

# Add to PATH if not already there
export PATH="$HOME/.local/bin:$PATH"
```

**Options**: `--version v0.1.0` (pin a version), `--upgrade` (force re-download),
`--install-dir /opt/bin` (custom location).

<details>
<summary>Manual install (no bootstrap script)</summary>

```bash
# 1. Pick your target from:
#    x86_64-unknown-linux-musl  |  aarch64-unknown-linux-musl
#    x86_64-apple-darwin        |  aarch64-apple-darwin
#    x86_64-pc-windows-gnu
# 2. Download + verify + install:
curl -L -o codespacectl.tar.gz \
  https://github.com/topic-hash/codespacectl/releases/latest/download/codespacectl-<target>.tar.gz
# Verify SHA-256 against SHA256SUMS.txt from the same release page
tar xzf codespacectl.tar.gz
install -m 0755 codespacectl ~/.local/bin/codespacectl
```
</details>

### Step 2 — Authenticate

`codespacectl` needs a GitHub PAT to talk to the Codespaces API. There are
two ways to provide it, in order of precedence:

```bash
# Option A: Environment variable (recommended for CI / agents / one-off use)
export CODESPACECTL_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxx

# Option B: Persisted token file (for interactive use)
echo -n 'ghp_xxxxxxxxxxxxxxxxxxxx' | codespacectl token set
# Stored at ~/.config/codespacectl/token with 0600 permissions
```

**Token scopes required:**

| Action | Scopes |
|---|---|
| Discover, connect, exec, stop | `codespace` |
| Push git commits from inside the codespace | `repo` (plus `codespace`) |

**Verify the token works:**

```bash
codespacectl discover          # should list your codespaces (or empty list)
codespacectl token get        # should print the token file path
```

If `discover` returns auth errors, the token is invalid, expired, or missing
the `codespace` scope. Regenerate it at `https://github.com/settings/tokens`.

### Step 3 — Discover Your Codespaces

List all codespaces accessible to your token:

```bash
codespacectl discover
```

Output:

```
#    NAME                                        STATE          REPO                             CREATED
------------------------------------------------------------------------------------------------------------------------
*  1  symmetrical-tribble-pjvp5rjg5w5v299jq       Available     topic-hash/DataMigrata            2026-01-15T00:00:00Z
   2  psychic-space-fishstick-gxrwv4rprvrcwjwv    Shutdown      topic-hash/three-pillars-voip     2026-01-10T00:00:00Z

(*) = current codespace
```

**Filter by repo:**

```bash
codespacectl discover --repo DataMigrata
```

**Filter by state:**

```bash
codespacectl discover --state Available
```

**Machine-readable output (for agent parsing):**

```bash
codespacectl discover --json
```

Note the full codespace name (e.g. `symmetrical-tribble-pjvp5rjg5w5v299jq`) — you
will need it for `connect` and `stop`.

### Step 4 — Select a Codespace

If you have multiple codespaces, set the current one before connecting:

```bash
# By full or partial name (first match wins)
codespacectl switch --codespace symmetrical-tribble-pjvp5rjg5w5v299jq

# By index from discover output
codespacectl switch --index 1
```

`switch` only updates the local state pointer — it does **not** connect or
start the codespace. It is safe to call repeatedly (idempotent).

### Step 5 — Connect

This is the core command. It does everything in one shot:

1. Starts the codespace if it is stopped (waits up to `--timeout` seconds)
2. Establishes SSH via `gh cs ssh --stdio` (no system SSH required)
3. Performs TOFU host-key verification
4. Loads the `CODESPACE.yaml` manifest (if present)
5. Runs `postStart` hooks from the manifest
6. Runs health checks from the manifest

```bash
codespacectl connect --codespace symmetrical-tribble-pjvp5rjg5w5v299jq \
  --accept-new-host-key --timeout 300
```

**Flags:**

| Flag | When to use |
|---|---|
| `--accept-new-host-key` | **Always on first connect.** Also after the codespace is rebuilt (host key rotates). |
| `--timeout <secs>` | Default is 180s. Increase to 300–600 for codespaces with heavy containers. |
| `--skip-health` | Debugging only — skips health checks when you know the codespace is healthy. |
| `--skip-hooks` | Debugging only — skips `postStart` hooks. |

**Common gotchas:**

| Symptom | Cause | Fix |
|---|---|---|
| `host key mismatch` | Codespace was rebuilt, or first connect without `--accept-new-host-key` | Re-run with `--accept-new-host-key` |
| `SSH handshake failed: Disconnected` | Codespace just started, SSH daemon still booting | Wait 20 seconds, re-run `connect` |
| `codespace_start_timeout` | Cold start taking too long (heavy Docker images, large devcontainers) | Increase `--timeout 600` and retry |
| `token_invalid_scope` | PAT missing `codespace` scope | Regenerate PAT with `codespace` scope |
| `binary_missing` | `gh` CLI not found on PATH | Install `gh`, or set `CODESPACECTL_GH_BIN` env var |

**Verify connection succeeded:**

```bash
codespacectl state           # should show current codespace and manifest
codespacectl doctor          # should show all checks OK
```

### Step 6 — Run Commands

Once connected, run commands on the codespace. Two modes:

#### Ad-hoc commands (`raw`)

No manifest needed. No health gate. Just a shell command over SSH.

```bash
codespacectl raw "cd /workspaces/my-repo && pwd && git log -1 --oneline"
codespacectl raw "docker ps"
codespacectl raw "uname -a"
codespacectl raw "cat /etc/os-release"
```

Every command runs in a fresh SSH exec channel — there is no persistent shell
session, so `cd` does not persist between calls. Always use `cd /path && command`
in a single string.

**Timeout:** Default is 300s. Override with `--timeout`:

```bash
codespacectl raw "cd /workspaces/my-repo && cargo build --release" --timeout 600
```

**Machine-readable output:**

```bash
codespacectl raw "df -h /workspaces" --json
```

#### Manifest commands (`exec`)

If a `CODESPACE.yaml` manifest is loaded (from `connect`), run named commands
defined in the manifest's `commands` section. These run health gates before
executing.

```bash
# Runs health checks first, then executes the command
codespacectl exec test
codespacectl exec build

# Skip health gate (for debugging)
codespacectl exec test --force

# Override timeout
codespacectl exec test --timeout 600
```

#### Example `CODESPACE.yaml`

```yaml
apiVersion: v1
metadata:
  name: my-app
  description: My app's codespace operations

environment:
  workingDir: /workspaces/my-app
  healthChecks:
    - name: docker
      command: docker info
      expectExitCode: 0
      timeoutSecs: 10

commands:
  test:
    command: cd {{workingDir}} && make test
    timeoutSecs: 300
    requiresHealth: [docker]
  build:
    command: cd {{workingDir}} && make build
    timeoutSecs: 300

hooks:
  postStart:
    - command: docker compose up -d
      cwd: "{{workingDir}}"
      timeoutSecs: 120
  preStop:
    - command: docker compose down
      cwd: "{{workingDir}}"
      timeoutSecs: 30
```

Register the manifest before connecting:

```bash
codespacectl init ./CODESPACE.yaml
```

Or let `connect` auto-discover it at the repo root.

### Step 7 — Diagnose Problems

When something goes wrong, use these commands in order:

```bash
# 1. Check environment (token, gh binary, network, state file)
codespacectl doctor

# 2. Check connection state
codespacectl state

# 3. Run health checks only
codespacectl health

# 4. Browse session logs for detailed error traces
codespacectl session log --last 5
codespacectl session log --session <session-id>
```

**Read errors from `--json` output.** Every error includes:
- `kind`: machine-readable error type (18 possible kinds)
- `retryable`: whether to retry
- `suggested_action`: what to do next
- `context`: structured error details

```bash
codespacectl exec test --json
# If it fails, parse error.kind and error.suggested_action
```

### Step 8 — Stop the Codespace

When finished, stop the codespace to save compute hours:

```bash
codespacectl stop --codespace symmetrical-tribble-pjvp5rjg5w5v299jq
```

This runs `preStop` hooks from the manifest (e.g. `docker compose down`),
then calls the GitHub API to shut down the codespace. If no `--codespace` flag
is given, it uses the current codespace from state.

**Skip preStop hooks** (e.g. you already ran them manually):

```bash
codespacectl stop --skip-hooks
```

### Step 9 — Revoke the Token (when done)

If you are done for good and want to clean up:

```bash
codespacectl token clear
unset CODESPACECTL_TOKEN
```

---

## Workflow Summary

For agents and scripts, the complete lifecycle is:

```
bootstrap.sh → token set → discover → switch → connect → raw/exec → stop → token clear
```

| Phase | Command | Purpose |
|---|---|---|
| Install | `bash scripts/bootstrap.sh` | Get the binary (zero network from clone) |
| Auth | `codespacectl token set < file` | Store PAT |
| Discover | `codespacectl discover --json` | List available codespaces |
| Select | `codespacectl switch --codespace <name>` | Set current codespace in state |
| Connect | `codespacectl connect --codespace <name> --accept-new-host-key --timeout 300` | Start + SSH + hooks + health |
| Execute | `codespacectl raw "<cmd>"` or `codespacectl exec <name>` | Run commands on the codespace |
| Diagnose | `codespacectl doctor` / `codespacectl state` / `codespacectl health` | Debug problems |
| Stop | `codespacectl stop --codespace <name>` | Shut down the codespace |
| Cleanup | `codespacectl token clear` | Remove local token |

---

## JSON Envelope

Every command supports `--json` for structured output. This is how agents
should consume all output.

```bash
$ codespacectl exec test --json
{
  "schema": "codespacectl/v1",
  "ok": true,
  "result": {
    "command_name": "test",
    "exit_code": 0,
    "duration_secs": 12.4
  },
  "error": null,
  "warnings": [],
  "session": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "log_path": "~/.cache/codespacectl/sessions/550e8400.ndjson"
  }
}
```

On error:

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
    "context": { "check": "docker", "exit_code": 1 }
  }
}
```

---

## Documentation

| Document | Description |
|---|---|
| [Manifest Specification](docs/MANIFEST_SPEC.md) | `CODESPACE.yaml` schema — all fields, types, defaults |
| [CLI Reference](docs/CLI_REFERENCE.md) | Every subcommand, every flag, JSON schemas, error catalog (18 kinds), exit codes, env vars |
| [Architecture](docs/ARCHITECTURE.md) | Internal design for contributors |
| [CI Setup](docs/SETUP_CI.md) | GitHub Actions integration guide |

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `CODESPACECTL_TOKEN` | (unset) | GitHub PAT. Takes precedence over the token file. |
| `CODESPACECTL_GH_BIN` | (unset) | Path to `gh` binary. Skips PATH lookup if set. |
| `GH_TOKEN` | (unset) | Fallback token for `gh cs ssh --stdio` subprocess. |
| `XDG_CACHE_HOME` | `~/.cache` | Moves state, sessions, SSH keys. |
| `XDG_CONFIG_HOME` | `~/.config` | Moves token file. |

---

## License

MIT
