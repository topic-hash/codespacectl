//! Codespace API methods on `GitHubClient`.

use super::GitHubClient;
use crate::{CodespaceError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

/// Subset of codespace state values returned by the GitHub API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CodespaceState {
    Unknown,
    Created,
    Queued,
    Provisioning,
    Available,
    Starting,
    ShuttingDown,
    Shutdown,
    Failed,
    Exporting,
    Updating,
    Deleted,
}

impl Default for CodespaceState {
    fn default() -> Self {
        Self::Unknown
    }
}

impl std::fmt::Display for CodespaceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "Unknown"),
            Self::Created => write!(f, "Created"),
            Self::Queued => write!(f, "Queued"),
            Self::Provisioning => write!(f, "Provisioning"),
            Self::Available => write!(f, "Available"),
            Self::Starting => write!(f, "Starting"),
            Self::ShuttingDown => write!(f, "ShuttingDown"),
            Self::Shutdown => write!(f, "Shutdown"),
            Self::Failed => write!(f, "Failed"),
            Self::Exporting => write!(f, "Exporting"),
            Self::Updating => write!(f, "Updating"),
            Self::Deleted => write!(f, "Deleted"),
        }
    }
}

impl CodespaceState {
    pub fn from_str(s: &str) -> Self {
        match s {
            "Unknown" => Self::Unknown,
            "Created" => Self::Created,
            "Queued" => Self::Queued,
            "Provisioning" => Self::Provisioning,
            "Available" => Self::Available,
            "Starting" => Self::Starting,
            "ShuttingDown" => Self::ShuttingDown,
            "Shutdown" => Self::Shutdown,
            "Failed" => Self::Failed,
            "Exporting" => Self::Exporting,
            "Updating" => Self::Updating,
            "Deleted" => Self::Deleted,
            _ => Self::Unknown,
        }
    }
}

/// Subset of the codespace info returned by the GitHub API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodespaceInfo {
    pub name: String,
    pub state: CodespaceState,
    pub repository: CodespaceRepo,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub display_name: Option<String>,
    pub machine: Option<CodespaceMachine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodespaceRepo {
    pub full_name: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodespaceMachine {
    pub display_name: String,
    pub cpus: u32,
    pub memory_in_bytes: u64,
}

/// Response from `GET /user/codespaces`.
#[derive(Debug, Deserialize)]
struct ListCodespacesResponse {
    codespaces: Vec<CodespaceInfo>,
    total_count: u32,
}

impl GitHubClient {
    /// List all codespaces for the authenticated user.
    pub async fn list_codespaces(&self) -> Result<Vec<CodespaceInfo>> {
        let resp = self
            .request(reqwest::Method::GET, "/user/codespaces")
            .send()
            .await?;
        let resp = self.map_error(resp).await?;
        let parsed: ListCodespacesResponse = resp.json().await?;
        Ok(parsed.codespaces)
    }

    /// Get info about a specific codespace by name.
    pub async fn get_codespace(&self, name: &str) -> Result<CodespaceInfo> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/user/codespaces/{}", name),
            )
            .send()
            .await?;
        let resp = self.map_error(resp).await?;
        let parsed: CodespaceInfo = resp.json().await?;
        Ok(parsed)
    }

    /// Start a codespace (transition from Shutdown to Available).
    pub async fn start_codespace(&self, name: &str) -> Result<()> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/user/codespaces/{}/start", name),
            )
            .send()
            .await?;
        // GitHub returns 200 with the codespace info, or 202 if still starting
        let status = resp.status();
        if !status.is_success() {
            let _ = self.map_error(resp).await?;
        }
        Ok(())
    }

    /// Stop a codespace (transition from Available to Shutdown).
    pub async fn stop_codespace(&self, name: &str) -> Result<()> {
        let resp = self
            .request(
                reqwest::Method::POST,
                &format!("/user/codespaces/{}/stop", name),
            )
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let _ = self.map_error(resp).await?;
        }
        Ok(())
    }

    /// Wait for a codespace to reach the desired state, polling every 5 seconds.
    pub async fn wait_for_state(
        &self,
        name: &str,
        target: CodespaceState,
        timeout_secs: u64,
    ) -> Result<CodespaceInfo> {
        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut attempts = 0u32;
        let mut last_state = CodespaceState::Unknown;

        loop {
            attempts += 1;
            let info = self.get_codespace(name).await?;
            last_state = info.state.clone();
            if info.state == target {
                return Ok(info);
            }
            if std::time::Instant::now() >= deadline {
                return Err(CodespaceError::CodespaceStartTimeout {
                    elapsed_secs: (attempts * 5) as u64,
                });
            }
            sleep(Duration::from_secs(5)).await;
        }
    }

    /// Start a codespace and wait for it to become Available.
    /// Used by `connect` to ensure the codespace is ready.
    pub async fn ensure_running(&self, name: &str, timeout_secs: u64) -> Result<CodespaceInfo> {
        let info = self.get_codespace(name).await?;
        match info.state {
            CodespaceState::Available => Ok(info),
            CodespaceState::Shutdown | CodespaceState::ShuttingDown => {
                self.start_codespace(name).await?;
                self.wait_for_state(name, CodespaceState::Available, timeout_secs).await
            }
            CodespaceState::Starting | CodespaceState::Provisioning | CodespaceState::Queued => {
                self.wait_for_state(name, CodespaceState::Available, timeout_secs).await
            }
            other => Err(CodespaceError::CodespaceUnreachable(format!(
                "codespace {} is in state {} — cannot ensure running",
                name, other
            ))),
        }
    }
}
