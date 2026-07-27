//! Trait abstractions over the GitHub API + shell execution.
//!
//! These traits are the "ports" in Clean Architecture terms. Use cases (Wave 2)
//! receive `&dyn GithubApiClient` as a parameter rather than calling the concrete
//! `GitHubClient` directly, enabling test fakes and future alternative impls
//! (e.g. GitHub Enterprise Server with different auth).

use crate::github::codespaces::{CodespaceInfo, CodespaceState};
use crate::Result;
use async_trait::async_trait;

/// Port for all GitHub Codespaces API operations.
///
/// Implementations:
/// - `GitHubClient` (production — uses reqwest + rustls)
/// - `FakeGithubApiClient` (tests — in-memory state, in `test_support`)
#[async_trait]
pub trait GithubApiClient: Send + Sync {
    /// Validate the token by calling /user. Returns the login username on success.
    async fn validate_token(&self) -> Result<String>;

    /// List all codespaces for the authenticated user.
    async fn list_codespaces(&self) -> Result<Vec<CodespaceInfo>>;

    /// Get info about a specific codespace by name.
    async fn get_codespace(&self, name: &str) -> Result<CodespaceInfo>;

    /// Start a codespace (transition from Shutdown to Available).
    async fn start_codespace(&self, name: &str) -> Result<()>;

    /// Stop a codespace (transition from Available to Shutdown).
    async fn stop_codespace(&self, name: &str) -> Result<()>;

    /// Wait for a codespace to reach the desired state, polling every 5 seconds.
    async fn wait_for_state(
        &self,
        name: &str,
        target: CodespaceState,
        timeout_secs: u64,
    ) -> Result<CodespaceInfo>;

    /// Convenience: ensure codespace is Available (start if needed, wait).
    async fn ensure_running(&self, name: &str, timeout_secs: u64) -> Result<CodespaceInfo>;
}

/// Port for spawning subprocesses (specifically the gh CLI for SSH transport).
///
/// Wave 1 only defines the trait — Wave 2 will refactor `ssh/transport.rs` to
/// use it instead of calling `tokio::process::Command::new` directly.
#[async_trait]
pub trait ShellExecutor: Send + Sync {
    /// Spawn a command, returning the child's stdout as a String.
    /// Used for things like `gh --version` in `doctor`.
    async fn run(&self, program: &str, args: &[&str]) -> Result<String>;
}

// -------------------------------------------------------------------------
// Test support — public test fakes for use-case unit tests (Wave 2+).
//
// Importing this module from a sibling crate's `#[cfg(test)] mod tests` block
// gives tests access to deterministic in-memory impls of `GithubApiClient`
// and `ShellExecutor` without needing to mock HTTP or subprocess I/O.
//
// All items here are `#[cfg(test)]`-gated, so they compile out of release
// builds entirely.
// -------------------------------------------------------------------------

#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::github::codespaces::{
        CodespaceInfo, CodespaceMachine, CodespaceRepo, CodespaceState,
    };
    use crate::{CodespaceError, Result};
    use std::sync::Mutex;

    /// Build a minimal `CodespaceInfo` for tests, with the given name + state.
    /// The repository is hardcoded to `owner/repo` (sufficient for tests that
    /// don't care about repo filtering).
    pub fn make_info(name: &str, state: CodespaceState) -> CodespaceInfo {
        CodespaceInfo {
            name: name.into(),
            state,
            repository: CodespaceRepo {
                full_name: "owner/repo".into(),
                name: "repo".into(),
            },
            created_at: "2024-01-01T00:00:00Z".into(),
            last_used_at: None,
            display_name: None,
            machine: Some(CodespaceMachine {
                display_name: "small".into(),
                cpus: 2,
                memory_in_bytes: 4 * 1024 * 1024 * 1024,
            }),
        }
    }

    /// Like `make_info` but lets the caller specify the repository `full_name`
    /// (e.g. `"topic-hash/DataMigrata"`). Used by tests that filter by repo
    /// substring.
    pub fn make_info_with_repo(
        name: &str,
        state: CodespaceState,
        repo_full_name: &str,
    ) -> CodespaceInfo {
        // Derive the repo short-name from the full name (last segment after `/`).
        let short_name = repo_full_name
            .split('/')
            .last()
            .filter(|s| !s.is_empty())
            .unwrap_or("repo");
        CodespaceInfo {
            name: name.into(),
            state,
            repository: CodespaceRepo {
                full_name: repo_full_name.into(),
                name: short_name.into(),
            },
            created_at: "2024-01-01T00:00:00Z".into(),
            last_used_at: None,
            display_name: None,
            machine: Some(CodespaceMachine {
                display_name: "small".into(),
                cpus: 2,
                memory_in_bytes: 4 * 1024 * 1024 * 1024,
            }),
        }
    }

    // ---------------------------------------------------------------------
    // FakeGithubApiClient — in-memory implementation of GithubApiClient.
    // ---------------------------------------------------------------------

    /// In-memory fake for `GithubApiClient`. Holds a `Vec<CodespaceInfo>`
    /// protected by a `Mutex` so it's `Send + Sync`.
    ///
    /// State transitions on `start_*` / `stop_*` are immediate (no async
    /// polling needed), making this suitable for deterministic unit tests.
    ///
    /// Wave 2 use-case tests construct one with `FakeGithubApiClient::new(seeded)`,
    /// pass it to the use-case function as `&dyn GithubApiClient`, and assert
    /// on the result. The `codespaces` field is `pub` so tests can also
    /// introspect or mutate the seeded state directly when needed.
    pub struct FakeGithubApiClient {
        pub codespaces: Mutex<Vec<CodespaceInfo>>,
    }

    impl FakeGithubApiClient {
        /// Construct with a seeded list of codespaces.
        pub fn new(seeded: Vec<CodespaceInfo>) -> Self {
            Self {
                codespaces: Mutex::new(seeded),
            }
        }

        /// Helper: mutate the codespace with the given name in place.
        fn mutate<F>(&self, name: &str, f: F) -> Result<()>
        where
            F: FnOnce(&mut CodespaceInfo),
        {
            let mut guard = self.codespaces.lock().unwrap();
            let info = guard
                .iter_mut()
                .find(|c| c.name == name)
                .ok_or_else(|| CodespaceError::CodespaceNotFound(name.into()))?;
            f(info);
            Ok(())
        }

        /// Helper: clone out the codespace with the given name.
        fn get(&self, name: &str) -> Result<CodespaceInfo> {
            let guard = self.codespaces.lock().unwrap();
            guard
                .iter()
                .find(|c| c.name == name)
                .cloned()
                .ok_or_else(|| CodespaceError::CodespaceNotFound(name.into()))
        }
    }

    #[async_trait]
    impl GithubApiClient for FakeGithubApiClient {
        async fn validate_token(&self) -> Result<String> {
            Ok("fake-user".into())
        }

        async fn list_codespaces(&self) -> Result<Vec<CodespaceInfo>> {
            Ok(self.codespaces.lock().unwrap().clone())
        }

        async fn get_codespace(&self, name: &str) -> Result<CodespaceInfo> {
            self.get(name)
        }

        async fn start_codespace(&self, name: &str) -> Result<()> {
            self.mutate(name, |info| info.state = CodespaceState::Available)
        }

        async fn stop_codespace(&self, name: &str) -> Result<()> {
            self.mutate(name, |info| info.state = CodespaceState::Shutdown)
        }

        async fn wait_for_state(
            &self,
            name: &str,
            target: CodespaceState,
            _timeout_secs: u64,
        ) -> Result<CodespaceInfo> {
            // Fakes are synchronous — just check the current state matches.
            let info = self.get(name)?;
            if info.state == target {
                Ok(info)
            } else {
                Err(CodespaceError::CodespaceStartTimeout { elapsed_secs: 0 })
            }
        }

        async fn ensure_running(&self, name: &str, timeout_secs: u64) -> Result<CodespaceInfo> {
            let info = self.get(name)?;
            match info.state {
                CodespaceState::Available => Ok(info),
                CodespaceState::Shutdown | CodespaceState::ShuttingDown => {
                    GithubApiClient::start_codespace(self, name).await?;
                    GithubApiClient::wait_for_state(
                        self,
                        name,
                        CodespaceState::Available,
                        timeout_secs,
                    )
                    .await
                }
                _ => {
                    GithubApiClient::wait_for_state(
                        self,
                        name,
                        CodespaceState::Available,
                        timeout_secs,
                    )
                    .await
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // FakeShellExecutor — records spawn calls, returns a seeded string.
    // ---------------------------------------------------------------------

    /// Minimal fake for `ShellExecutor`. Returns a hardcoded stdout string
    /// regardless of input, and records the call args so tests can assert
    /// the right program was invoked. The `calls` field is `pub` for
    /// post-call inspection.
    pub struct FakeShellExecutor {
        pub seeded_stdout: String,
        pub calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl FakeShellExecutor {
        pub fn new(seeded_stdout: &str) -> Self {
            Self {
                seeded_stdout: seeded_stdout.into(),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ShellExecutor for FakeShellExecutor {
        async fn run(&self, program: &str, args: &[&str]) -> Result<String> {
            self.calls.lock().unwrap().push((
                program.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            Ok(self.seeded_stdout.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for in-memory fake impls of the trait ports. These fakes are the
    //! foundation for Wave 2's use-case-level tests (which inject them as
    //! dependencies). Wave 1 only verifies the fakes themselves behave
    //! correctly — no use-case logic is exercised here.

    use super::test_support::*;
    use super::*;
    use crate::CodespaceError;

    #[tokio::test]
    async fn test_fake_list_codespaces_returns_seeded_state() {
        let seeded = vec![
            make_info("alpha", CodespaceState::Available),
            make_info("beta", CodespaceState::Shutdown),
        ];
        let fake = FakeGithubApiClient::new(seeded);
        let listed = fake.list_codespaces().await.expect("list should succeed");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "alpha");
        assert_eq!(listed[0].state, CodespaceState::Available);
        assert_eq!(listed[1].name, "beta");
        assert_eq!(listed[1].state, CodespaceState::Shutdown);
    }

    #[tokio::test]
    async fn test_fake_start_codespace_flips_state_to_available() {
        let seeded = vec![make_info("alpha", CodespaceState::Shutdown)];
        let fake = FakeGithubApiClient::new(seeded);
        fake.start_codespace("alpha")
            .await
            .expect("start should succeed");
        let info = fake
            .get_codespace("alpha")
            .await
            .expect("get should succeed");
        assert_eq!(info.state, CodespaceState::Available);
    }

    #[tokio::test]
    async fn test_fake_stop_codespace_flips_state_to_shutdown() {
        let seeded = vec![make_info("alpha", CodespaceState::Available)];
        let fake = FakeGithubApiClient::new(seeded);
        fake.stop_codespace("alpha")
            .await
            .expect("stop should succeed");
        let info = fake
            .get_codespace("alpha")
            .await
            .expect("get should succeed");
        assert_eq!(info.state, CodespaceState::Shutdown);
    }

    #[tokio::test]
    async fn test_fake_get_codespace_returns_matching_entry_or_not_found() {
        let seeded = vec![make_info("alpha", CodespaceState::Available)];
        let fake = FakeGithubApiClient::new(seeded);

        // Happy path: existing codespace.
        let info = fake
            .get_codespace("alpha")
            .await
            .expect("existing codespace should be returned");
        assert_eq!(info.name, "alpha");

        // Sad path: missing codespace.
        let err = fake
            .get_codespace("nonexistent")
            .await
            .expect_err("missing codespace should error");
        assert!(
            matches!(err, CodespaceError::CodespaceNotFound(_)),
            "expected CodespaceNotFound, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_make_info_with_repo_sets_full_name_and_short_name() {
        let info =
            make_info_with_repo("alpha", CodespaceState::Available, "topic-hash/DataMigrata");
        assert_eq!(info.repository.full_name, "topic-hash/DataMigrata");
        assert_eq!(info.repository.name, "DataMigrata");
    }

    #[tokio::test]
    async fn test_fake_shell_executor_returns_seeded_stdout() {
        let fake = FakeShellExecutor::new("gh version 2.40.0 (2024-01-01)\n");
        let out = fake
            .run("gh", &["--version"])
            .await
            .expect("run should succeed");
        assert_eq!(out, "gh version 2.40.0 (2024-01-01)\n");

        // Assert the call was recorded.
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "exactly one call should be recorded");
        assert_eq!(calls[0].0, "gh");
        assert_eq!(calls[0].1, vec!["--version"]);
    }
}
