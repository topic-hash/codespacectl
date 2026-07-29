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

## Quick Start

### Install

No sudo required. The bootstrap script resolves a binary through a tiered
lookup — bundled pre-compiled binaries first (zero network), then local install,
then cache, then GitHub Releases download with SHA-256 verification.

```bash
curl -fsSL https://github.com/topic-hash/codespacectl/raw/main/scripts/bootstrap.sh | bash
# Add ~/.local/bin to PATH if not already there:
#   echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
```

Options: pin a version (`--version v0.1.0`), force re-download (`--upgrade`),
or override the install location (`--install-dir /opt/bin`).

**Agent-friendly:** If you have the repo cloned (e.g. in a sandbox), running the
bootstrap script from `scripts/bootstrap.sh` installs from the bundled binary
in ~40ms with zero network calls. If the installed binary is for the wrong
platform (e.g. sandbox switched from x86_64 to arm64), it is automatically
removed and replaced with the correct one.

<details>
<summary>Manual install (no bootstrap script)</summary>

```bash
# 1. Pick your target: x86_64-unknown-linux-musl | aarch64-unknown-linux-musl
#    | x86_64-apple-darwin | aarch64-apple-darwin | x86_64-pc-windows-gnu
# 2. Download from the latest release:
curl -L -o codespacectl.tar.gz \
  https://github.com/topic-hash/codespacectl/releases/latest/download/codespacectl-<target>.tar.gz
# 3. Verify the SHA-256 against SHA256SUMS.txt from the same release.
# 4. Extract and install:
tar xzf codespacectl.tar.gz
install -m 0755 codespacectl ~/.local/bin/codespacectl
```
</details>

### Set your GitHub token

Generate a fine-grained PAT with `codespace` scope (and `repo` if pushing),
then:

```bash
export CODESPACECTL_TOKEN=ghp_xxx
```

### Create a `CODESPACE.yaml` in your repo

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

### Use it

```bash
codespacectl connect --codespace <name>   # start, hooks, health check
codespacectl exec test                   # from CODESPACE.yaml
codespacectl exec build
codespacectl stop
```

## JSON Envelope

Every command supports `--json` for structured output:

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

## Documentation

- [Manifest Specification](docs/MANIFEST_SPEC.md)
- [CLI Reference](docs/CLI_REFERENCE.md) (includes Error Catalog)
- [Architecture](docs/ARCHITECTURE.md)

## License

MIT
