You are working on the `topic-hash/DataMigrata` repository — a Rust + SQL data-migration toolkit that runs against a live MSSQL 2022 Docker database inside a GitHub Codespace. All work happens inside the codespace, not on the host. Use the `codespacectl` CLI to connect and run every command remotely.

## Environment (host machine — read-only)

- `codespacectl` binary: `/home/z/.local/bin/codespacectl` (already on PATH)
- Token: `~/.config/codespacectl/token` (already configured)
- SSH key: `~/.ssh/codespaces.auto` (already configured)
- Local repo mirror (read-only reference, do all git work inside the codespace): `/home/z/my-project/DataMigrata-repo/`
- Local manifest: `/home/z/my-project/CODESPACE.yaml` — do not modify or remove.

## Codespace identity

- Display name: `symmetrical-tribble`
- Full machine name: `symmetrical-tribble-pjvp5rjg5w5v299jq`
- Repository: https://github.com/topic-hash/DataMigrata
- Working directory inside codespace: `/workspaces/DataMigrata`
- Docker compose directory: `/workspaces/DataMigrata/docker`

## Connect via codespacectl

Run these on the host. `codespacectl discover/switch/connect` is idempotent — safe to re-run if the session drops.

**Required first step** — set `GH_TOKEN` env var. The PAT in `~/.config/codespacectl/token` is valid (curl returns HTTP 200) but lacks `read:org` scope, so `gh auth login --with-token` rejects it. `codespacectl` works fine when `GH_TOKEN` is set explicitly:

```bash
export PATH="/home/z/.local/bin:$PATH"
export GH_TOKEN="$(cat ~/.config/codespacectl/token)"
```

If `codespacectl connect` reports `host key mismatch: expected unknown, got SSH handshake failed: Disconnected`, the codespace was likely just started and SSH is still booting — wait 20s and retry with `--accept-new-host-key`. If MSSQL queries fail with `Login timeout expired` immediately after `docker compose up`, the SQL Server service inside the container is still starting — wait ~20s and retry.

```bash
# 1. Make the codespace current in state (already current if `codespacectl state` shows it)
codespacectl switch --codespace symmetrical-tribble-pjvp5rjg5w5v299jq

# 2. Connect — starts the codespace if stopped, runs postStart hooks, runs health checks
codespacectl connect --codespace symmetrical-tribble-pjvp5rjg5w5v299jq \
  --accept-new-host-key --timeout 300

# 3. If health fails, diagnose before retrying:
codespacectl doctor
codespacectl state
# Use --skip-health only as a last resort for debugging.
```

## Run all subsequent commands inside the codespace

Use `codespacectl raw "<shell command>"` for ad-hoc shell, or `codespacectl exec <name>` for manifest-declared commands. Always `cd /workspaces/DataMigrata` first.

```bash
# Verify working copy
codespacectl raw "cd /workspaces/DataMigrata && pwd && git log -1 --oneline && git status"

# Start MSSQL Docker (if not already running)
codespacectl raw "cd /workspaces/DataMigrata/docker && docker compose up -d && docker compose ps"

# Check container
codespacectl raw "docker ps --filter name=mssql-advanced-demo"
```

## MSSQL connection details (inside the codespace)

- Container name: `mssql-advanced-demo` (MSSQL 2022 RTM-CU26)
- Database: `MSSQL_Advanced_Demo`
- sqlcmd path inside container: `/opt/mssql-tools18/bin/sqlcmd`
- Connection string: `-S localhost -U sa -P YourStrong@Passw0rd -C`
- Existing data: 5,000 employees + 5,000 transactions (see `sql/01_MSSQL_Migration_SyntheticData.sql`)

```bash
# Run a SQL file
codespacectl raw "cd /workspaces/DataMigrata && docker exec -i mssql-advanced-demo /opt/mssql-tools18/bin/sqlcmd -S localhost -U sa -P 'YourStrong@Passw0rd' -C -i sql/00_COMPLETE_MSSQL_Deployment.sql"

# Run an ad-hoc query
codespacectl raw "docker exec -i mssql-advanced-demo /opt/mssql-tools18/bin/sqlcmd -S localhost -U sa -P 'YourStrong@Passw0rd' -C -Q 'SELECT DB_NAME(), @@VERSION'"

# Run the 50-op verification suite (split + per-op runner is in scripts/mssql_runner/)
codespacectl raw "cd /workspaces/DataMigrata && python3 scripts/mssql_runner/split_and_run.py"
```

## Repo layout

```
/workspaces/DataMigrata/
├── sql/
│   ├── 00_COMPLETE_MSSQL_Deployment.sql      # full schema + seed data
│   ├── 00_SCHEMA_ONLY_Deployment.sql         # schema only, no data
│   ├── 01_MSSQL_Migration_SyntheticData.sql  # 5k employees + 5k transactions
│   ├── 02_MSSQL_50_Operations_Expanded.sql   # the 50 ops under test
│   └── 02_MSSQL_50_Sophisticated_Operations.sql
├── scripts/
│   ├── mssql_runner/split_and_run.py         # splits 02_… by `-- OP N:` headers, runs each via sqlcmd
│   ├── ops/op_01.sql … op_50.sql             # per-op split files
│   └── patches/                               # DDL patch files written during wave-1 schema fixes
├── docker/                                    # docker-compose.yml for mssql-advanced-demo
├── tools/
│   ├── bin/gh                                 # static x86_64 gh CLI v2.63.2
│   └── codespace_ssh.py                       # legacy paramiko-based SSH (fallback only)
├── RESULTS_50_OPS.md                          # verified 50/50 PASS — commit 7f5b6de
├── SETUP.md
├── AGENT_CODESPACE_PROMPT.md                  # legacy paramiko bootstrap (ignore — codespacectl replaces this)
└── worklog.md                                 # multi-agent worklog — READ FIRST before starting any task
```

## Git operations (inside the codespace)

The codespace's git credential helper is already authenticated to `topic-hash/DataMigrata`.

```bash
codespacectl raw "cd /workspaces/DataMigrata && git add -A && git commit -m '<message>'"
codespacectl raw "cd /workspaces/DataMigrata && git push origin main"
codespacectl raw "cd /workspaces/DataMigrata && git pull --rebase origin main"
```

## Workflow rules

1. **Always read `worklog.md` first** — it carries the full history of prior agent runs, schema fixes, and known-good states. Append your own section to it when you finish a task (use the `---\nTask ID: …\nAgent: …\nTask: …\nWork Log: …\nStage Summary: …` template).
2. **The current known-good state is commit `7f5b6de`** — `50/50 ops PASS, 0 FAIL, 0 TIMEOUT`. If you change schema or ops, re-run `split_and_run.py` and confirm 50/50 still passes before pushing.
3. **MSSQL schema patches must be idempotent** — every `CREATE` preceded by an `IF OBJECT_ID … DROP` guard in its own `GO`-separated batch. Schema-qualified names everywhere. No `ORDER BY` inside view definitions (Msg 1033). No `INSERT/UPDATE/DELETE` in patch files unless the task explicitly requires it.
4. **Do not edit files on the host mirror** at `/home/z/my-project/DataMigrata-repo/` — it is a read-only reference. All edits go through `codespacectl raw` against `/workspaces/DataMigrata/`.
5. **Stop the codespace when finished** to save compute:
   ```bash
   codespacectl stop --codespace symmetrical-tribble-pjvp5rjg5w5v299jq
   ```

## Fallback: legacy paramiko SSH

If `codespacectl` is broken (e.g., you are mid-refactor of codespacectl itself), fall back to the paramiko toolchain documented in `AGENT_CODESPACE_PROMPT.md` at the repo root. Use the same PAT already in `~/.config/codespacectl/token` — do NOT paste a literal token into the prompt or commit history (GitHub Push Protection will reject the push). If the token has expired, generate a new fine-grained PAT at https://github.com/settings/tokens with scopes `codespace` + `repo`, then `echo -n '<token>' > ~/.config/codespacectl/token`.
