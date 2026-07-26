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
    /// Total count of codespaces (per GitHub API spec). Not used by codespacectl
    /// — we use `codespaces.len()` instead — but the field must be present for
    /// serde deserialization to succeed when the API returns it.
    #[allow(dead_code)]
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

        loop {
            attempts += 1;
            let info = self.get_codespace(name).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::GitHubClient;

    // -------------------------------------------------------------------------
    // CodespaceState — from_str / Display / Default / serde round-trips.
    // -------------------------------------------------------------------------

    #[test]
    fn test_state_from_str_all_variants() {
        assert_eq!(CodespaceState::from_str("Unknown"), CodespaceState::Unknown);
        assert_eq!(CodespaceState::from_str("Created"), CodespaceState::Created);
        assert_eq!(CodespaceState::from_str("Queued"), CodespaceState::Queued);
        assert_eq!(CodespaceState::from_str("Provisioning"), CodespaceState::Provisioning);
        assert_eq!(CodespaceState::from_str("Available"), CodespaceState::Available);
        assert_eq!(CodespaceState::from_str("Starting"), CodespaceState::Starting);
        assert_eq!(CodespaceState::from_str("ShuttingDown"), CodespaceState::ShuttingDown);
        assert_eq!(CodespaceState::from_str("Shutdown"), CodespaceState::Shutdown);
        assert_eq!(CodespaceState::from_str("Failed"), CodespaceState::Failed);
        assert_eq!(CodespaceState::from_str("Exporting"), CodespaceState::Exporting);
        assert_eq!(CodespaceState::from_str("Updating"), CodespaceState::Updating);
        assert_eq!(CodespaceState::from_str("Deleted"), CodespaceState::Deleted);
    }

    #[test]
    fn test_state_from_str_unknown_string_returns_unknown() {
        assert_eq!(CodespaceState::from_str("NotARealState"), CodespaceState::Unknown);
    }

    #[test]
    fn test_state_from_str_empty_string_returns_unknown() {
        assert_eq!(CodespaceState::from_str(""), CodespaceState::Unknown);
    }

    #[test]
    fn test_state_display_pascalcase() {
        assert_eq!(CodespaceState::Unknown.to_string(), "Unknown");
        assert_eq!(CodespaceState::Created.to_string(), "Created");
        assert_eq!(CodespaceState::Queued.to_string(), "Queued");
        assert_eq!(CodespaceState::Provisioning.to_string(), "Provisioning");
        assert_eq!(CodespaceState::Available.to_string(), "Available");
        assert_eq!(CodespaceState::Starting.to_string(), "Starting");
        assert_eq!(CodespaceState::ShuttingDown.to_string(), "ShuttingDown");
        assert_eq!(CodespaceState::Shutdown.to_string(), "Shutdown");
        assert_eq!(CodespaceState::Failed.to_string(), "Failed");
        assert_eq!(CodespaceState::Exporting.to_string(), "Exporting");
        assert_eq!(CodespaceState::Updating.to_string(), "Updating");
        assert_eq!(CodespaceState::Deleted.to_string(), "Deleted");
    }

    #[test]
    fn test_state_default_is_unknown() {
        assert_eq!(CodespaceState::default(), CodespaceState::Unknown);
    }

    #[test]
    fn test_state_serialize_produces_pascalcase() {
        let json = serde_json::to_string(&CodespaceState::ShuttingDown).unwrap();
        assert_eq!(json, "\"ShuttingDown\"");
        let json = serde_json::to_string(&CodespaceState::Available).unwrap();
        assert_eq!(json, "\"Available\"");
    }

    #[test]
    fn test_state_deserialize_accepts_pascalcase() {
        let s: CodespaceState = serde_json::from_str("\"Available\"").unwrap();
        assert_eq!(s, CodespaceState::Available);
        let s: CodespaceState = serde_json::from_str("\"ShuttingDown\"").unwrap();
        assert_eq!(s, CodespaceState::ShuttingDown);
    }

    #[test]
    fn test_state_round_trip_serialize_deserialize() {
        for state in [
            CodespaceState::Unknown,
            CodespaceState::Created,
            CodespaceState::Queued,
            CodespaceState::Provisioning,
            CodespaceState::Available,
            CodespaceState::Starting,
            CodespaceState::ShuttingDown,
            CodespaceState::Shutdown,
            CodespaceState::Failed,
            CodespaceState::Exporting,
            CodespaceState::Updating,
            CodespaceState::Deleted,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: CodespaceState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back, "round-trip failed for {:?}", state);
        }
    }

    // -------------------------------------------------------------------------
    // API method tests via mockito.
    //
    // Each test spins up a fresh mockito server, points a GitHubClient at it,
    // and exercises one API method.
    // -------------------------------------------------------------------------

    /// Build a tiny codespace JSON body matching `CodespaceInfo`.
    fn codespace_body(name: &str, state: &str) -> String {
        format!(
            r#"{{
                "name": "{}",
                "state": "{}",
                "repository": {{"full_name": "owner/repo", "name": "repo"}},
                "created_at": "2026-01-01T00:00:00Z",
                "last_used_at": null,
                "display_name": null,
                "machine": null
            }}"#,
            name, state
        )
    }

    async fn make_client(server_url: String) -> GitHubClient {
        GitHubClient::new_with_base_url("ghp_test".into(), server_url).unwrap()
    }

    // -------------------- list_codespaces --------------------

    #[tokio::test]
    async fn test_list_codespaces_parses_valid_response() {
        let mut server = mockito::Server::new_async().await;
        let body = format!(
            r#"{{"total_count": 2, "codespaces": [{}, {}]}}"#,
            codespace_body("cs-1", "Available"),
            codespace_body("cs-2", "Shutdown")
        );
        let m = server
            .mock("GET", "/user/codespaces")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let list = client.list_codespaces().await.expect("list should succeed");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "cs-1");
        assert_eq!(list[0].state, CodespaceState::Available);
        assert_eq!(list[1].name, "cs-2");
        assert_eq!(list[1].state, CodespaceState::Shutdown);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_codespaces_empty_response() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/user/codespaces")
            .with_status(200)
            .with_body(r#"{"total_count": 0, "codespaces": []}"#)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let list = client.list_codespaces().await.expect("list should succeed");
        assert!(list.is_empty());
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_codespaces_total_count_handled_correctly() {
        // The list method only returns the codespaces array; total_count is
        // parsed by the deserializer but otherwise ignored. Verify that a
        // mismatch between total_count and array length doesn't break parsing.
        let mut server = mockito::Server::new_async().await;
        let body = format!(
            r#"{{"total_count": 99, "codespaces": [{}]}}"#,
            codespace_body("only-one", "Available")
        );
        let m = server
            .mock("GET", "/user/codespaces")
            .with_status(200)
            .with_body(body)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let list = client.list_codespaces().await.expect("list should succeed");
        assert_eq!(list.len(), 1);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_codespaces_errors_on_401() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/user/codespaces")
            .with_status(401)
            .with_body(r#"{"message":"Bad credentials"}"#)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let err = client.list_codespaces().await.unwrap_err();
        assert!(
            matches!(err, CodespaceError::TokenRevoked),
            "401 Bad credentials should map to TokenRevoked, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_list_codespaces_errors_on_403() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/user/codespaces")
            .with_status(403)
            .with_body(r#"{"message":"Resource not accessible by token"}"#)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let err = client.list_codespaces().await.unwrap_err();
        assert!(
            matches!(err, CodespaceError::TokenInvalidScope { .. }),
            "403 should map to TokenInvalidScope, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_list_codespaces_errors_on_500() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/user/codespaces")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let err = client.list_codespaces().await.unwrap_err();
        assert!(
            matches!(err, CodespaceError::CodespaceUnreachable(_)),
            "500 should map to CodespaceUnreachable, got {:?}",
            err
        );
    }

    // -------------------- get_codespace --------------------

    #[tokio::test]
    async fn test_get_codespace_parses_single_response() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/user/codespaces/my-cs")
            .with_status(200)
            .with_body(codespace_body("my-cs", "Available"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let info = client.get_codespace("my-cs").await.expect("get should succeed");
        assert_eq!(info.name, "my-cs");
        assert_eq!(info.state, CodespaceState::Available);
        assert_eq!(info.repository.full_name, "owner/repo");
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_codespace_errors_on_404() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/user/codespaces/missing")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let err = client.get_codespace("missing").await.unwrap_err();
        assert!(
            matches!(err, CodespaceError::CodespaceNotFound(_)),
            "404 should map to CodespaceNotFound, got {:?}",
            err
        );
    }

    // -------------------- start_codespace --------------------

    #[tokio::test]
    async fn test_start_codespace_succeeds_on_200() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/user/codespaces/cs/start")
            .with_status(200)
            .with_body(codespace_body("cs", "Starting"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        client.start_codespace("cs").await.expect("start should succeed on 200");
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_start_codespace_succeeds_on_202() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/user/codespaces/cs/start")
            .with_status(202)
            .with_body("")
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        client.start_codespace("cs").await.expect("start should succeed on 202");
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_start_codespace_errors_on_404() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/user/codespaces/missing/start")
            .with_status(404)
            .with_body(r#"{"message":"Not Found"}"#)
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let err = client.start_codespace("missing").await.unwrap_err();
        assert!(
            matches!(err, CodespaceError::CodespaceNotFound(_)),
            "404 should map to CodespaceNotFound, got {:?}",
            err
        );
    }

    // -------------------- stop_codespace --------------------

    #[tokio::test]
    async fn test_stop_codespace_succeeds_on_200() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/user/codespaces/cs/stop")
            .with_status(200)
            .with_body(codespace_body("cs", "ShuttingDown"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        client.stop_codespace("cs").await.expect("stop should succeed on 200");
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_stop_codespace_succeeds_on_202() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/user/codespaces/cs/stop")
            .with_status(202)
            .with_body("")
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        client.stop_codespace("cs").await.expect("stop should succeed on 202");
        m.assert_async().await;
    }

    // -------------------- wait_for_state --------------------

    #[tokio::test]
    async fn test_wait_for_state_returns_immediately_when_target_met() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Available"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let info = client
            .wait_for_state("cs", CodespaceState::Available, 30)
            .await
            .expect("should return immediately");
        assert_eq!(info.state, CodespaceState::Available);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_wait_for_state_polls_until_target_reached() {
        // Register two mocks for the same path. mockito's matching logic
        // gives priority to mocks that haven't yet hit their `expect` count,
        // so the first mock (returns "Starting", expect(2)) is used for the
        // first two polls, after which it's satisfied and the second mock
        // (returns "Available") takes over.
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Starting"))
            .expect(2)
            .create_async()
            .await;
        let m2 = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Available"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        // The polling loop sleeps 5s between attempts — we can't easily skip
        // the sleep without modifying production code, so we wrap the whole
        // thing in a 20s outer timeout to keep the test bounded.
        let info = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.wait_for_state("cs", CodespaceState::Available, 60),
        )
        .await
        .expect("test itself should not time out")
        .expect("wait_for_state should eventually succeed");
        assert_eq!(info.state, CodespaceState::Available);
        m.assert_async().await;
        m2.assert_async().await;
    }

    #[tokio::test]
    async fn test_wait_for_state_errors_on_timeout() {
        let mut server = mockito::Server::new_async().await;
        // Always returns "Starting" — never reaches target.
        let _m = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Starting"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        // Use a tiny timeout (1 second) so the test completes quickly. The
        // first poll returns "Starting" (not target), then the deadline (now
        // +1s) is already past, so we return CodespaceStartTimeout.
        let err = client
            .wait_for_state("cs", CodespaceState::Available, 1)
            .await
            .unwrap_err();
        assert!(
            matches!(err, CodespaceError::CodespaceStartTimeout { .. }),
            "should timeout with CodespaceStartTimeout, got {:?}",
            err
        );
    }

    // -------------------- ensure_running --------------------

    #[tokio::test]
    async fn test_ensure_running_returns_immediately_when_available() {
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Available"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let info = client
            .ensure_running("cs", 30)
            .await
            .expect("should return immediately when Available");
        assert_eq!(info.state, CodespaceState::Available);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_ensure_running_calls_start_when_shutdown() {
        let mut server = mockito::Server::new_async().await;
        // First GET returns Shutdown, then start is POSTed, then the next GET
        // (during wait_for_state) returns Available.
        let m_get1 = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Shutdown"))
            .expect(1)
            .create_async()
            .await;
        let m_post = server
            .mock("POST", "/user/codespaces/cs/start")
            .with_status(202)
            .with_body("")
            .create_async()
            .await;
        let m_get2 = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Available"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let info = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.ensure_running("cs", 60),
        )
        .await
        .expect("test itself should not time out")
        .expect("ensure_running should succeed");
        assert_eq!(info.state, CodespaceState::Available);
        m_get1.assert_async().await;
        m_post.assert_async().await;
        m_get2.assert_async().await;
    }

    #[tokio::test]
    async fn test_ensure_running_waits_when_starting() {
        let mut server = mockito::Server::new_async().await;
        // First GET returns Starting, second GET (during wait_for_state)
        // returns Available.
        let m_get1 = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Starting"))
            .expect(1)
            .create_async()
            .await;
        let m_get2 = server
            .mock("GET", "/user/codespaces/cs")
            .with_status(200)
            .with_body(codespace_body("cs", "Available"))
            .create_async()
            .await;

        let client = make_client(server.url()).await;
        let info = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            client.ensure_running("cs", 60),
        )
        .await
        .expect("test itself should not time out")
        .expect("ensure_running should succeed");
        assert_eq!(info.state, CodespaceState::Available);
        m_get1.assert_async().await;
        m_get2.assert_async().await;
    }
}
