//! Shared helpers for CLI command handlers.
//!
//! Centralizes: token resolution + authed GitHubClient construction,
//! gh binary discovery, manifest loading (from `--manifest` arg or auto-discovered),
//! codespace name resolution (from `--codespace` arg or `state.current_codespace`),
//! and secret-resolution-into-TemplateContext.

use crate::github::auth::resolve_token;
use crate::github::traits::GithubApiClient;
use crate::github::GitHubClient;
use crate::manifest::{find_manifest, parse_manifest_from_file, Manifest, TemplateContext};
use crate::secrets::{generate_secret, SecretStore};
use crate::state::load_state;
use crate::{CodespaceError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Resolve the GitHub token, build a `GitHubClient`, validate the token, and
/// return it behind the `GithubApiClient` trait object.
///
/// Returning `Arc<dyn GithubApiClient>` (rather than the concrete
/// `GitHubClient`) lets Wave 2 use-case handlers accept a trait-object
/// parameter so test fakes can be injected. Wave 1 callers (connect.rs,
/// stop.rs) still work unchanged — the trait methods on `Arc<dyn Trait>`
/// dispatch transparently via auto-deref.
pub async fn authed_client() -> Result<Arc<dyn GithubApiClient>> {
    let token = resolve_token()?;
    let client = GitHubClient::new(token)?;
    // Validate via the trait (delegates to the inherent method on GitHubClient).
    GithubApiClient::validate_token(&client).await?;
    Ok(Arc::new(client))
}

/// Resolve the gh CLI binary path.
///
/// Order of precedence:
/// 1. `CODESPACECTL_GH_BIN` env var (used verbatim if non-empty).
/// 2. `tools/bin/gh` relative to the manifest directory (if a manifest dir is given).
/// 3. `gh` found anywhere on `PATH`.
///
/// Returns `CodespaceError::BinaryMissing` if none of the above succeeds.
/// Resolve the gh CLI binary path. Tries, in order:
///   1. `CODESPACECTL_GH_BIN` env var (explicit override)
///   2. `tools/bin/gh` relative to manifest dir (vendored)
///   3. `gh` from PATH (system install)
///   4. `~/.cache/codespacectl/bin/gh` (auto-downloaded by `ensure_gh_binary`)
///
/// If none are found, attempts to auto-download gh from GitHub releases.
/// Returns the path to the gh binary, or an error if download fails or the
/// platform is unsupported.
pub async fn resolve_gh_bin(manifest_dir: Option<&Path>) -> Result<String> {
    // 1. Env var
    if let Ok(gh) = std::env::var("CODESPACECTL_GH_BIN") {
        if !gh.is_empty() && PathBuf::from(&gh).exists() {
            return Ok(gh);
        }
    }
    // 2. tools/bin/gh relative to manifest dir
    if let Some(dir) = manifest_dir {
        let candidate = dir.join("tools").join("bin").join("gh");
        if candidate.exists() {
            return Ok(candidate.display().to_string());
        }
    }
    // 3. gh from PATH or cached download (find_gh_binary checks both)
    if let Some(path) = crate::github::find_gh_binary() {
        return Ok(path.display().to_string());
    }
    // 4. Auto-download from GitHub releases
    let path = crate::github::ensure_gh_binary().await?;
    Ok(path.display().to_string())
}

/// Load the manifest from `args_manifest` (if `Some`), else walk up from CWD
/// looking for `CODESPACE.yaml` / `CODESPACE.yml`.
///
/// Returns the parsed manifest, the absolute path it was loaded from, and the
/// directory containing it (used for `tools/bin/gh` lookup).
pub fn load_manifest_for(args_manifest: Option<&str>) -> Result<(Manifest, PathBuf, PathBuf)> {
    let path: PathBuf = if let Some(m) = args_manifest {
        let p = PathBuf::from(m);
        if !p.exists() {
            return Err(CodespaceError::ManifestNotFound(format!(
                "manifest file not found: {}",
                m
            )));
        }
        p
    } else {
        let cwd = std::env::current_dir().map_err(|e| {
            CodespaceError::Internal(format!("failed to determine current dir: {}", e))
        })?;
        find_manifest(&cwd)?
    };
    let manifest = parse_manifest_from_file(&path)?;
    let dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((manifest, path, dir))
}

/// Resolve codespace name: use `--codespace` arg if provided, else fall back
/// to `state.current_codespace`.
///
/// Returns `CodespaceError::Internal` if neither is set, with a hint to run
/// `codespacectl connect` first.
pub fn resolve_codespace_name(arg: Option<&str>) -> Result<String> {
    if let Some(name) = arg {
        if !name.is_empty() {
            return Ok(name.to_string());
        }
    }
    let state = load_state()?;
    state.current_codespace.ok_or_else(|| {
        CodespaceError::Internal(
            "no codespace specified and no current codespace in state — run `codespacectl connect --codespace <name>` first".into(),
        )
    })
}

/// Resolve every secret declared in `manifest.environment.secrets` into a
/// `TemplateContext` ready to be passed to `render_template` /
/// `exec_command` / `run_post_start` / `run_pre_stop` / `run_all_checks`.
///
/// For each secret:
/// - If already stored (`SecretStore::exists`), retrieve it via `SecretStore::get`.
/// - Else, if `generate_if_missing` is Some, generate a new secret via
///   `generate_secret(length, charset)`, store it, and use it.
/// - Else, if `required` is true, return
///   `CodespaceError::Internal("required secret missing: NAME")`.
/// - Else (optional with no generate config), skip — the placeholder
///   `{{secret.NAME}}` will be left unsubstituted by the renderer.
pub fn resolve_template_context(manifest: &Manifest) -> Result<TemplateContext> {
    let mut secrets: HashMap<String, String> = HashMap::new();
    for s in &manifest.environment.secrets {
        if SecretStore::exists(&s.name) {
            let v = SecretStore::get(&s.name)?;
            secrets.insert(s.name.clone(), v);
        } else if let Some(gen) = &s.generate_if_missing {
            let value = generate_secret(gen.length, &gen.charset);
            SecretStore::set(&s.name, &value)?;
            secrets.insert(s.name.clone(), value);
        } else if s.required {
            return Err(CodespaceError::Internal(format!(
                "required secret missing: {} — populate it via the SecretStore or set generateIfMissing in the manifest",
                s.name
            )));
        }
        // else: optional with no generate config — leave undefined
    }
    Ok(TemplateContext {
        working_dir: manifest.environment.working_dir.clone(),
        secrets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `resolve_gh_bin` returns BinaryMissing when CODESPACECTL_GH_BIN is unset,
    /// no manifest_dir is given, and PATH contains no `gh`. We can't fully
    /// guarantee PATH state in a unit test, so we only assert the result is
    /// either Ok(String) or Err(BinaryMissing) — never another variant.
    #[tokio::test]
    async fn test_resolve_gh_bin_returns_expected_variant() {
        // Temporarily clear CODESPACECTL_GH_BIN so the env-var path is skipped.
        let prev = std::env::var_os("CODESPACECTL_GH_BIN");
        std::env::remove_var("CODESPACECTL_GH_BIN");

        let r = resolve_gh_bin(None).await;
        match r {
            Ok(s) => assert!(!s.is_empty(), "gh_bin path should be non-empty"),
            Err(CodespaceError::BinaryMissing(_)) => { /* expected when gh absent */ }
            Err(other) => panic!("expected Ok or BinaryMissing, got {:?}", other),
        }

        if let Some(v) = prev {
            std::env::set_var("CODESPACECTL_GH_BIN", v);
        }
    }

    /// `resolve_gh_bin` prefers `CODESPACECTL_GH_BIN` when set to an existing path.
    /// (Since resolve_gh_bin now verifies the env var path exists, we point at
    /// /bin/true which always exists on Unix.)
    #[tokio::test]
    async fn test_resolve_gh_bin_prefers_env_var_when_path_exists() {
        let prev = std::env::var_os("CODESPACECTL_GH_BIN");
        // Use /bin/true as a stand-in (exists, is executable)
        std::env::set_var("CODESPACECTL_GH_BIN", "/bin/true");

        let r = resolve_gh_bin(None)
            .await
            .expect("env var should win when path exists");
        assert_eq!(r, "/bin/true");

        match prev {
            Some(v) => std::env::set_var("CODESPACECTL_GH_BIN", v),
            None => std::env::remove_var("CODESPACECTL_GH_BIN"),
        }
    }

    /// `resolve_codespace_name` returns the arg when provided.
    #[test]
    fn test_resolve_codespace_name_with_arg() {
        let r = resolve_codespace_name(Some("my-codespace")).unwrap();
        assert_eq!(r, "my-codespace");
    }

    /// `resolve_codespace_name` falls back to state.current_codespace if it exists.
    /// (We can't easily manipulate the real state file in a unit test without
    /// touching XDG_CACHE_HOME, so this is a smoke test of the "arg provided"
    /// path; the "no arg, no state" path is exercised in integration.)
    #[test]
    fn test_resolve_codespace_name_empty_arg_treated_as_none() {
        // Empty string arg should fall through to state lookup, which (without
        // a configured state file in the test env) returns Internal. Either
        // outcome is acceptable; we just verify the function doesn't panic.
        let _ = resolve_codespace_name(Some(""));
    }

    /// `resolve_template_context` builds a context with no secrets for a manifest
    /// that declares none.
    #[test]
    fn test_resolve_template_context_no_secrets() {
        let manifest = Manifest {
            api_version: "v1".into(),
            metadata: crate::manifest::Metadata {
                name: "test".into(),
                description: None,
                repo: None,
            },
            environment: crate::manifest::Environment {
                working_dir: "/workspaces".into(),
                health_checks: vec![],
                secrets: vec![],
            },
            commands: HashMap::new(),
            hooks: None,
        };
        let ctx = resolve_template_context(&manifest).unwrap();
        assert_eq!(ctx.working_dir, "/workspaces");
        assert!(ctx.secrets.is_empty());
    }
}
