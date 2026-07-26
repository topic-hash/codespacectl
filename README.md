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

```bash
curl -L https://github.com/topic-hash/codespacectl/releases/latest/download/codespacectl-linux-amd64 \
  -o /usr/local/bin/codespacectl && chmod +x $_
```

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
