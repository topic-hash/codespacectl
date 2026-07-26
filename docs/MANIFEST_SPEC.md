# CODESPACE.yaml — Manifest Specification

This is the **authoritative** specification for the `CODESPACE.yaml` file
format consumed by `codespacectl`. The implementation lives in
`src/manifest/schema.rs` and `src/manifest/parser.rs`; if the implementation
and this document disagree, **this document is wrong** (file an issue).

- **Status**: `v1` (stable)
- **Schema identifier**: `codespacectl/v1`
- **Last updated**: 2026-01

---

## 1. Overview

`CODESPACE.yaml` is a single declarative file that lives at the **root** of a
repository. It tells `codespacectl`:

- where the codespace's working directory is (`environment.workingDir`),
- what health checks must pass before commands run (`environment.healthChecks`),
- what commands can be invoked by name (`commands`),
- what lifecycle hooks to run on connect / stop (`hooks`), and
- what secrets the project needs, and how to materialize them
  (`environment.secrets`).

A typical project has exactly one `CODESPACE.yaml`. `codespacectl` walks up
from the current directory until it finds one (see §11 — Discovery).

### 1.1 Where it lives

- Default location: `<repo-root>/CODESPACE.yaml`
- Also accepted (fallback): `<repo-root>/CODESPACE.yml`
- Override: `codespacectl --manifest /path/to/CODESPACE.yaml ...`
- URL registration: `codespacectl init https://example.com/CODESPACE.yaml`

### 1.2 Why YAML

YAML was chosen over JSON or TOML for the same reason Kubernetes and GitHub
Actions use it: humans hand-edit it, and it supports multi-line strings and
comments. The parser uses `serde_yaml` 0.9, which is strict about types but
tolerant about ordering, whitespace, and trailing comments.

---

## 2. Top-level schema

```yaml
apiVersion: v1
metadata:      Metadata        # required
environment:  Environment     # required
commands:     Map<name, Command>  # optional, default {}
hooks:        Hooks           # optional, default null
```

| Field         | Type                  | Required | Default | Description                                                |
|---------------|-----------------------|----------|---------|------------------------------------------------------------|
| `apiVersion`  | string                | yes      | —       | Manifest schema version. Must be `"v1"`.                  |
| `metadata`    | `Metadata`            | yes      | —       | Human-readable identification of the project.             |
| `environment` | `Environment`         | yes      | —       | Working directory, health checks, secrets.                 |
| `commands`    | `Map<string, Command>`| no       | `{}`    | Named commands invokable via `codespacectl exec <name>`.  |
| `hooks`       | `Hooks`               | no       | `null`  | `postStart` / `preStop` lifecycle hooks.                 |

---

## 3. `Metadata`

```yaml
metadata:
  name: data-migrata
  description: Data migration tooling for SQL Server → Postgres
  repo: topic-hash/DataMigrata
```

| Field          | Type   | Required | Default | Description                                                            |
|----------------|--------|----------|---------|------------------------------------------------------------------------|
| `name`         | string | yes      | —       | Stable identifier. Must match `^[a-z0-9-]+$`. Used in the session log. |
| `description`  | string | no       | `null`  | Free-form short description. Surfaced by `codespacectl state`.        |
| `repo`         | string | no       | `null`  | `owner/repo` slug (informational only; not used for API calls).        |

Validation rules (enforced by `validate_manifest`):

- `name` must be non-empty and match `^[a-z0-9-]+$` (lowercase ASCII
  letters, digits, and hyphens only — this matches GitHub Codespace name
  conventions so the manifest `name` can be used as a filesystem-safe label).
- `description` and `repo` may be any string but should be short.

---

## 4. `Environment`

```yaml
environment:
  workingDir: /workspaces/DataMigrata
  healthChecks:
    - name: docker
      command: docker info
      expectExitCode: 0
      timeoutSecs: 10
  secrets:
    - name: SA_PASSWORD
      required: true
      generateIfMissing:
        length: 32
        charset: alnum+symbols
```

| Field           | Type            | Required | Default          | Description                              |
|-----------------|-----------------|----------|------------------|------------------------------------------|
| `workingDir`    | string          | no       | `"/workspaces"`  | Absolute path inside the codespace.     |
| `healthChecks`  | `Vec<HealthCheck>` | no    | `[]`             | Ordered list of health checks.           |
| `secrets`       | `Vec<Secret>`   | no       | `[]`             | Secrets the project needs at exec time. |

Validation rules:

- `workingDir` **must** be an absolute path (start with `/`). This is
  checked by `validate_manifest`; relative paths are rejected because they
  would be evaluated against an unknown CWD on the codespace host.
- All `healthChecks[].name` values must be unique within the manifest.
- All `secrets[].name` values must be unique within the manifest.
- Every `commands.<name>.requiresHealth[i]` must reference a `healthChecks`
  entry that exists.

---

## 5. `HealthCheck`

```yaml
- name: docker
  command: docker info
  expectExitCode: 0
  timeoutSecs: 10
```

| Field            | Type    | Required | Default | Description                                            |
|------------------|---------|----------|---------|--------------------------------------------------------|
| `name`           | string  | yes      | —       | Unique check identifier (also used by `requiresHealth`). |
| `command`        | string  | yes      | —       | Shell command run over SSH. Supports template syntax.  |
| `expectExitCode` | integer | no       | `0`     | Exit code that means "pass".                           |
| `timeoutSecs`    | integer | no       | `30`    | SSH exec timeout.                                      |

### 5.1 Health check semantics

- The check is run by sending `command` to the codespace via the SSH exec
  channel (see [ARCHITECTURE.md](./ARCHITECTURE.md)).
- The `command` is rendered through the template engine first, so
  `{{workingDir}}` and `{{secret.NAME}}` substitutions are applied.
- The remote exit code is compared against `expectExitCode`. A match means
  **pass**; anything else means **fail**.
- If the SSH transport returns a *soft* error (timeout, exec failure, channel
  closed without exit status), the check is recorded as **failed** with
  `exit_code = -1`, and iteration continues to the next check so partial
  results can still be collected.
- If the SSH transport returns a *hard* error (`CodespaceUnreachable`,
  `HostKeyMismatch`, `NetworkError`, or transport closed), iteration stops
  immediately and the error propagates to the caller. See `error.rs` and
  the [CLI_REFERENCE.md](./CLI_REFERENCE.md) error catalog.
- The aggregate result is `green` iff every check passed; otherwise `red`.
- An empty `healthChecks` list is **trivially green**.

---

## 6. `Secret`

```yaml
secrets:
  - name: SA_PASSWORD
    required: true
    generateIfMissing:
      length: 32
      charset: alnum+symbols
  - name: OPTIONAL_API_KEY
    required: false
```

| Field                | Type              | Required | Default      | Description                                                |
|----------------------|-------------------|----------|--------------|------------------------------------------------------------|
| `name`               | string            | yes      | —            | Stable identifier, used in `{{secret.NAME}}` placeholders. |
| `required`           | boolean           | no       | `false`      | If true, missing secret = error.                          |
| `generateIfMissing`  | `GenerateConfig`  | no       | `null`       | If set, generate + store on first use.                    |

### 6.1 `GenerateConfig`

| Field     | Type    | Required | Default          | Description                                                    |
|-----------|---------|----------|------------------|----------------------------------------------------------------|
| `length`  | integer | no       | `24`             | Number of characters to generate.                             |
| `charset` | string  | no       | `"alnum+symbols"` | One of: `alnum`, `alnum+symbols`, `hex`, `base64`.           |

### 6.2 Secret semantics

#### Resolution order (`resolve_template_context`)

For each declared secret, in declaration order:

1. **Already stored?** If `~/.cache/codespacectl/secrets/<name>.age` exists,
   decrypt it with the age identity at `~/.config/codespacectl/identity.age`
   and use the value.
2. **Generate?** If not stored and `generateIfMissing` is set, generate a
   fresh random value, encrypt it at rest, and use it.
3. **Required?** If not stored and not generatable and `required: true`,
   return `CodespaceError::Internal("required secret missing: NAME")`.
4. **Optional?** If not stored and not generatable and `required: false`,
   the placeholder `{{secret.NAME}}` is **left unsubstituted** in templates.
   This is intentional — operators may pre-populate the secret store out of
   band (`SecretStore::set`) before running commands that need it.

#### Encryption at rest

- All secrets are written as ASCII-armored age blobs to
  `~/.cache/codespacectl/secrets/<name>.age` with `0600` permissions on Unix.
- The age identity is a single X25519 key generated lazily on first use at
  `~/.config/codespacectl/identity.age` (also `0600` on Unix).
- The identity file is the *only* thing needed to decrypt every secret; if
  it's deleted, all stored secrets become unrecoverable.
- `codespacectl` never prints secret values to stdout/stderr/JSON output.
  The `doctor` command only reports whether the identity file exists.

#### Lifecycle

- **First use**: `codespacectl` calls `SecretStore::init()` (in `common.rs`)
  which creates the secrets dir + identity file if either is missing.
- **Generation**: only happens once per secret name (on first exec that
  resolves the template context). Subsequent runs read the stored value.
- **Overwrite**: calling `SecretStore::set(name, value)` again replaces the
  stored value. There is no CLI subcommand for this today; agents/scripts
  must invoke the library directly or pre-write the file via age.
- **Deletion**: `SecretStore::delete(name)` removes the encrypted blob. Same
  as above — no CLI subcommand yet; file removal also works.

#### Rotation

- If a codespace is rebuilt (detected via `created_at` newer than
  `host_key_stored_at`, see `state/codespace.rs`), the SSH host key is
  rotated but **secrets are not**. Secrets are scoped to the manifest, not
  to a specific codespace instance.
- To force re-generation of a secret, delete
  `~/.cache/codespacectl/secrets/<name>.age` and re-run a command that
  resolves the template context.

---

## 7. `Command`

```yaml
commands:
  test:
    description: Run the test suite
    command: cd {{workingDir}} && make test
    timeoutSecs: 300
    requiresHealth: [docker]
    idempotent: true
  build:
    command: cd {{workingDir}} && make build
    timeoutSecs: 600
```

| Field             | Type            | Required | Default | Description                                                |
|-------------------|-----------------|----------|---------|------------------------------------------------------------|
| `description`     | string          | no       | `null`  | Human-readable note (surfaced by `state`).                |
| `command`         | string          | yes      | —       | Shell command run via SSH. Supports template syntax.      |
| `timeoutSecs`     | integer         | no       | `300`   | SSH exec timeout.                                          |
| `requiresHealth`  | `Vec<string>`   | no       | `[]`    | Health check names that must be `green` for exec to run.  |
| `idempotent`      | boolean         | no       | `false` | Hint for callers — does *not* change `codespacectl` behavior. |

### 7.1 Command semantics

- The `command` field is rendered through the template engine (see §10).
- A non-zero remote exit code is **not** an error from `codespacectl`'s
  perspective. It is returned as `Ok(ExecOutput)` with `exit_code` set, and
  `codespacectl` propagates that exit code as its own process exit code.
- SSH transport errors and timeouts **are** errors (returned as `Err`).
- The `requiresHealth` gate is **only** enforced by `codespacectl exec`
  (see [CLI_REFERENCE.md](./CLI_REFERENCE.md)). `codespacectl raw` skips it
  (raw commands are ad-hoc).

---

## 8. `Hooks`

```yaml
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

| Field             | Type              | Required | Default | Description                          |
|-------------------|-------------------|----------|---------|--------------------------------------|
| `postStart`       | `Vec<HookCommand>`| no       | `[]`    | Run after `connect` + TOFU + health. |
| `preStop`         | `Vec<HookCommand>`| no       | `[]`    | Run before `stop` API call.         |

### 8.1 `HookCommand`

| Field         | Type    | Required | Default | Description                                            |
|---------------|---------|----------|---------|--------------------------------------------------------|
| `command`     | string  | yes      | —       | Shell command run via SSH. Supports template syntax.  |
| `cwd`         | string  | no       | `null`  | Working directory (rendered, then `cd <cwd> &&`).     |
| `timeoutSecs` | integer | no       | `300`   | SSH exec timeout.                                    |

### 8.2 Hook semantics

#### `postStart`

- Runs **after** `connect` has (a) brought the codespace to `Available`,
  (b) established the SSH session, (c) completed TOFU host-key verification,
  and (d) started the session log.
- Runs **before** the health check pass (so failed hooks short-circuit
  before health is checked).
- Each hook runs sequentially in declaration order.
- A non-zero exit code from any hook aborts the entire `connect` and
  returns `CodespaceError::CommandFailed`.
- A timeout returns `CodespaceError::CommandTimeout`.
- If `cwd` is provided, the rendered command is prefixed with
  `cd <rendered-cwd> && <rendered-command>`.
- `--skip-hooks` on `connect` bypasses this list entirely.

#### `preStop`

- Runs **before** the GitHub Codespaces API `stop` call.
- Each hook runs sequentially in declaration order.
- Non-zero exit code → `CodespaceError::CommandFailed`; the stop API call
  is **not** made (operator must fix the hook or use `--skip-hooks`).
- Timeout → `CodespaceError::CommandTimeout`; same — stop is skipped.
- `--skip-hooks` on `stop` bypasses this list entirely.

---

## 9. Validation rules (summary)

`validate_manifest` enforces the following invariants. Any violation
returns one of:

- `CodespaceError::ManifestVersionUnsupported` — `apiVersion != "v1"`
- `CodespaceError::ManifestInvalid` — any other schema/logic violation

| #   | Rule                                                                          |
|-----|-------------------------------------------------------------------------------|
| V1  | `apiVersion` must equal `"v1"`.                                              |
| V2  | `metadata.name` must be non-empty and match `^[a-z0-9-]+$`.                  |
| V3  | `environment.workingDir` must be absolute (start with `/`).                  |
| V4  | All `healthChecks[].name` values must be unique.                            |
| V5  | All `secrets[].name` values must be unique.                                 |
| V6  | Every `commands.<name>.requiresHealth[i]` must reference an existing check. |
| V7  | `commands.<name>` keys are unique (enforced by the `HashMap` deserializer). |
| V8  | Every required field above (`required: yes`) must be present.              |

Unknown fields in the YAML are silently ignored (forward compat). If you
want strict mode, file an issue.

---

## 10. Template syntax

The template engine (`manifest/templates.rs`) is deliberately minimal: it
performs plain-text substitution of `{{placeholder}}` tokens. There is no
expression language, no conditionals, no loops. This keeps the surface area
small for agents and avoids shell-injection footguns from a richer syntax.

### 10.1 Supported placeholders

| Placeholder            | Replaced with                                                    |
|------------------------|------------------------------------------------------------------|
| `{{workingDir}}`       | The manifest's `environment.workingDir` value (verbatim).        |
| `{{secret.NAME}}`      | The decrypted value of secret `NAME` (see §6).                   |

### 10.2 Pass-through rules

- Unknown placeholders (anything that doesn't match the two forms above) are
  **left as-is** in the output. This lets shell variables like `$HOME`,
  `${USER}`, `$$`, etc. pass through untouched.
- Substitution is **literal** — no escaping, no quoting. If a secret value
  contains shell metacharacters, the manifest author is responsible for
  quoting it in the command template (e.g. `echo '{{secret.TOKEN}}'`).
- Substitution is **case-sensitive**. `{{workingdir}}` will not match.

### 10.3 Examples

```yaml
# Working directory substitution
command: cd {{workingDir}} && cargo test

# Secret substitution (with shell quoting)
command: sqlcmd -S db -U sa -P '{{secret.SA_PASSWORD}}' -Q "SELECT 1"

# Shell variables pass through
command: cd {{workingDir}} && HOME=$HOME cargo build
```

---

## 11. Manifest discovery

When `--manifest` is not provided, `codespacectl` walks up from the current
directory looking for `CODESPACE.yaml` (or `CODESPACE.yml` as a fallback).
The first match wins. If none is found before reaching the filesystem root,
`CodespaceError::ManifestNotFound` is returned.

This mirrors `git`'s `.git/` discovery behavior, so `codespacectl exec test`
run from any subdirectory of a project works as expected.

---

## 12. Versioning policy

- The manifest schema is identified by the `apiVersion` field at the top
  of every `CODESPACE.yaml`.
- `apiVersion: v1` is the **current stable** schema. It will not receive
  breaking changes; new optional fields may be added in patch releases.
- `apiVersion: v2` (when introduced) will require an explicit migration:
  - Old `v1` manifests will continue to load under `codespacectl` versions
    that support `v2` (with a deprecation warning printed to stderr).
  - After one minor release cycle, `v1` support will be removed.
  - A `codespacectl migrate v1-to-v2` subcommand will be added to rewrite
    manifests in place.
- Unknown future `apiVersion` values return
  `CodespaceError::ManifestVersionUnsupported("<version>")` with
  `suggested_action = "Upgrade codespacectl, or use apiVersion: v1"`.

---

## 13. Complete example — Rust project (DataMigrata)

```yaml
# CODESPACE.yaml — DataMigrata
# Repo: topic-hash/DataMigrata
# Codespace: symmetrical-tribble-pjvp5rjg5w5v299jq
apiVersion: v1

metadata:
  name: data-migrata
  description: SQL Server → Postgres migration tooling
  repo: topic-hash/DataMigrata

environment:
  workingDir: /workspaces/DataMigrata
  healthChecks:
    - name: docker
      command: docker info
      expectExitCode: 0
      timeoutSecs: 10
    - name: cargo
      command: cd {{workingDir}} && cargo --version
      expectExitCode: 0
      timeoutSecs: 15
    - name: sql-server
      command: sqlcmd -S db -U sa -P '{{secret.SA_PASSWORD}}' -Q "SELECT 1"
      expectExitCode: 0
      timeoutSecs: 20

  secrets:
    - name: SA_PASSWORD
      required: true
      generateIfMissing:
        length: 32
        charset: alnum+symbols

commands:
  test:
    description: Run unit + integration tests
    command: cd {{workingDir}} && cargo test -- --test-threads=4
    timeoutSecs: 600
    requiresHealth: [docker, cargo]
    idempotent: true

  build:
    description: Release build
    command: cd {{workingDir}} && cargo build --release
    timeoutSecs: 900
    requiresHealth: [docker, cargo]

  migrate:
    description: Run the SQL Server → Postgres migration
    command: cd {{workingDir}} && cargo run --release -- migrate \
      --source 'Server=db;User Id=sa;Password={{secret.SA_PASSWORD}};' \
      --target 'postgres://postgres@localhost:5432/migrata'
    timeoutSecs: 1800
    requiresHealth: [docker, cargo, sql-server]

hooks:
  postStart:
    - command: docker compose up -d db
      cwd: "{{workingDir}}"
      timeoutSecs: 120
    - command: sqlcmd -S db -U sa -P '{{secret.SA_PASSWORD}}' -Q "WAITFOR DELAY '00:00:05'"
      cwd: "{{workingDir}}"
      timeoutSecs: 30
  preStop:
    - command: docker compose down
      cwd: "{{workingDir}}"
      timeoutSecs: 30
```

Usage:

```bash
codespacectl init ./CODESPACE.yaml
codespacectl connect --codespace symmetrical-tribble-pjvp5rjg5w5v299jq
codespacectl exec build
codespacectl exec migrate
codespacectl stop
```

---

## 14. Complete example — Python project (three-pillars-voip)

```yaml
# CODESPACE.yaml — three-pillars-voip
# Repo: topic-hash/three-pillars-voip
# Codespace: psychic-space-fishstick-gxrwv4rprvrcwjwv
apiVersion: v1

metadata:
  name: three-pillars-voip
  description: Three-pillars VoIP analysis dashboard
  repo: topic-hash/three-pillars-voip

environment:
  workingDir: /workspaces/three-pillars-voip
  healthChecks:
    - name: python
      command: python3 --version
      expectExitCode: 0
      timeoutSecs: 5
    - name: venv
      command: test -d {{workingDir}}/.venv
      expectExitCode: 0
      timeoutSecs: 5
    - name: redis
      command: redis-cli ping
      expectExitCode: 0
      timeoutSecs: 10

  secrets:
    - name: TWILIO_API_KEY
      required: true
      generateIfMissing:
        length: 40
        charset: alnum
    - name: OPTIONAL_SLACK_WEBHOOK
      required: false

commands:
  install:
    description: Create venv + install deps
    command: cd {{workingDir}} && python3 -m venv .venv && . .venv/bin/activate && pip install -r requirements.txt
    timeoutSecs: 600
    requiresHealth: [python]
    idempotent: true

  test:
    description: Run pytest suite
    command: cd {{workingDir}} && . .venv/bin/activate && pytest
    timeoutSecs: 600
    requiresHealth: [python, venv, redis]

  run-worker:
    description: Start the Celery worker (foreground)
    command: cd {{workingDir}} && . .venv/bin/activate && celery -A app worker --loglevel=info
    timeoutSecs: 3600
    requiresHealth: [python, venv, redis]

hooks:
  postStart:
    - command: docker run -d --name redis -p 6379:6379 redis:7
      cwd: "{{workingDir}}"
      timeoutSecs: 60
  preStop:
    - command: docker stop redis && docker rm redis
      cwd: "{{workingDir}}"
      timeoutSecs: 30
```

Usage:

```bash
codespacectl init ./CODESPACE.yaml
codespacectl connect --codespace psychic-space-fishstick-gxrwv4rprvrcwjwv
codespacectl exec install
codespacectl exec test
codespacectl exec run-worker
codespacectl stop
```
