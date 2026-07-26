//! HTTP client for GitHub API.

use crate::{CodespaceError, Result};
use reqwest::{Client, StatusCode};
use crate::github::codespaces::{CodespaceInfo, CodespaceState};
use crate::github::traits::GithubApiClient;

const API_BASE: &str = "https://api.github.com";

pub struct GitHubClient {
    pub client: Client,
    pub token: String,
    /// Base URL prefix prepended to relative paths in `request()`.
    /// Defaults to `https://api.github.com` but can be overridden via
    /// `new_with_base_url` (useful for GitHub Enterprise Server with a
    /// different API base, and for tests via `mockito`).
    pub base_url: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Result<Self> {
        Self::new_with_base_url(token, API_BASE.to_string())
    }

    /// Construct a `GitHubClient` pointed at a custom base URL.
    ///
    /// Useful for GitHub Enterprise Server installations (which expose the
    /// Codespaces API under a different host) and for tests using `mockito`.
    pub fn new_with_base_url(token: String, base_url: String) -> Result<Self> {
        let client = Client::builder()
            .user_agent("codespacectl/0.1 (https://github.com/topic-hash/codespacectl)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| CodespaceError::Internal(format!("reqwest client build failed: {}", e)))?;

        Ok(Self {
            client,
            token,
            base_url,
        })
    }

    /// Build a request with auth headers.
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        };
        self.client
            .request(method, &url)
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    /// Map a non-2xx response to a CodespaceError.
    pub async fn map_error(&self, resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let body = resp.text().await.unwrap_or_default();
        match status {
            StatusCode::UNAUTHORIZED => {
                if body.contains("Bad credentials") || body.contains("invalid") {
                    Err(CodespaceError::TokenRevoked)
                } else {
                    Err(CodespaceError::AuthFailed(body))
                }
            }
            StatusCode::FORBIDDEN => {
                if body.contains("scope") || body.contains("Resource not accessible") {
                    Err(CodespaceError::TokenInvalidScope {
                        scope: "codespace".into(),
                    })
                } else {
                    Err(CodespaceError::AuthFailed(format!("403: {}", body)))
                }
            }
            StatusCode::NOT_FOUND => Err(CodespaceError::CodespaceNotFound(body)),
            StatusCode::TOO_MANY_REQUESTS => {
                Err(CodespaceError::CodespaceUnreachable("rate limited".into()))
            }
            s if s.is_server_error() => {
                Err(CodespaceError::CodespaceUnreachable(format!("{}: {}", s, body)))
            }
            s => Err(CodespaceError::Internal(format!("HTTP {}: {}", s, body))),
        }
    }
}

// -------------------------------------------------------------------------
// Trait wiring — `GitHubClient` is the production impl of the
// `GithubApiClient` port. Each trait method delegates to the matching
// inherent `pub async fn` defined on `GitHubClient` itself (in `codespaces.rs`
// and `auth.rs`). Inherent methods take precedence over trait methods in Rust's
// name resolution, so there's no ambiguity / recursion.
//
// Wave 2 callers will receive `Arc<dyn GithubApiClient>` and dispatch via the
// trait; Wave 1's existing callers continue to use the inherent methods
// directly (they will be migrated in Wave 2).
// -------------------------------------------------------------------------

#[async_trait::async_trait]
impl GithubApiClient for GitHubClient {
    async fn validate_token(&self) -> Result<String> {
        // Delegates to the inherent `GitHubClient::validate_token` defined in `auth.rs`.
        self.validate_token().await
    }

    async fn list_codespaces(&self) -> Result<Vec<CodespaceInfo>> {
        self.list_codespaces().await
    }

    async fn get_codespace(&self, name: &str) -> Result<CodespaceInfo> {
        self.get_codespace(name).await
    }

    async fn start_codespace(&self, name: &str) -> Result<()> {
        self.start_codespace(name).await
    }

    async fn stop_codespace(&self, name: &str) -> Result<()> {
        self.stop_codespace(name).await
    }

    async fn wait_for_state(
        &self,
        name: &str,
        target: CodespaceState,
        timeout_secs: u64,
    ) -> Result<CodespaceInfo> {
        self.wait_for_state(name, target, timeout_secs).await
    }

    async fn ensure_running(&self, name: &str, timeout_secs: u64) -> Result<CodespaceInfo> {
        self.ensure_running(name, timeout_secs).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Constructor tests.
    // -------------------------------------------------------------------------

    #[test]
    fn test_new_with_valid_token_succeeds() {
        let client = GitHubClient::new("ghp_validtoken123".into());
        assert!(client.is_ok(), "valid token should construct OK");
        let client = client.unwrap();
        assert_eq!(client.token, "ghp_validtoken123");
    }

    #[test]
    fn test_new_with_empty_token_succeeds() {
        // Constructor doesn't validate token emptiness — validation happens
        // at API call time. So an empty token must construct OK.
        let client = GitHubClient::new(String::new());
        assert!(client.is_ok(), "empty token should still construct OK");
        assert_eq!(client.unwrap().token, "");
    }

    #[test]
    fn test_new_sets_default_base_url() {
        let client = GitHubClient::new("ghp_test".into()).unwrap();
        assert_eq!(client.base_url, "https://api.github.com");
    }

    #[test]
    fn test_new_with_base_url_sets_custom_base() {
        let client =
            GitHubClient::new_with_base_url("ghp_test".into(), "https://example.invalid".into())
                .unwrap();
        assert_eq!(client.base_url, "https://example.invalid");
    }

    #[test]
    fn test_new_sets_user_agent_header() {
        // We can't easily inspect the User-Agent on a RequestBuilder (it's
        // baked into the Client at construction time and not exposed
        // per-builder). Instead we verify that the Client itself was built
        // without error — the actual User-Agent value is exercised end-to-end
        // in the mockito-based tests below.
        let client = GitHubClient::new("ghp_test".into());
        assert!(client.is_ok());
    }

    // -------------------------------------------------------------------------
    // request() URL building + header tests via mockito.
    //
    // mockito lets us declare a mock for a path + headers; if the request
    // matches, the mock is hit and the test passes. If we set up a mock with
    // `match_header(...)` constraints and the client doesn't send the expected
    // header, mockito returns 404 (no matching mock) and we can detect that.
    // -------------------------------------------------------------------------

    /// Helper: spin up a mockito Server, return (server, base_url) where
    /// base_url is the URL the client should point at.
    async fn make_server() -> (mockito::ServerGuard, String) {
        let server = mockito::Server::new_async().await;
        let url = server.url();
        (server, url)
    }

    #[tokio::test]
    async fn test_request_prefixes_relative_path_with_base_url() {
        let (mut server, base) = make_server().await;
        let m = server
            .mock("GET", "/user/codespaces")
            .with_status(200)
            .with_body("{\"codespaces\":[],\"total_count\":0}")
            .create_async()
            .await;

        let client = GitHubClient::new_with_base_url("ghp_t".into(), base).unwrap();
        let resp = client
            .request(reqwest::Method::GET, "/user/codespaces")
            .send()
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_does_not_prefix_http_urls() {
        // Even with a base_url set, an absolute http:// URL must be used as-is.
        let (mut server, base) = make_server().await;
        let abs_url = format!("{}/user/codespaces", base);
        let m = server
            .mock("GET", "/user/codespaces")
            .with_status(200)
            .with_body("{\"codespaces\":[],\"total_count\":0}")
            .create_async()
            .await;

        let client = GitHubClient::new_with_base_url("ghp_t".into(), "https://wrong.invalid".into()).unwrap();
        let resp = client
            .request(reqwest::Method::GET, &abs_url)
            .send()
            .await
            .expect("absolute URL should be used as-is");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_does_not_prefix_https_urls() {
        let (mut server, base) = make_server().await;
        let abs_url = format!("{}/user", base);
        let m = server.mock("GET", "/user").with_status(200).with_body("{}").create_async().await;

        let client = GitHubClient::new_with_base_url("ghp_t".into(), "https://wrong.invalid".into()).unwrap();
        let resp = client.request(reqwest::Method::GET, &abs_url).send().await.expect("send");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_adds_authorization_header() {
        let (mut server, base) = make_server().await;
        let m = server
            .mock("GET", "/user")
            .match_header("authorization", "token ghp_secret_token")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = GitHubClient::new_with_base_url("ghp_secret_token".into(), base).unwrap();
        let resp = client.request(reqwest::Method::GET, "/user").send().await.expect("send");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_adds_accept_header() {
        let (mut server, base) = make_server().await;
        let m = server
            .mock("GET", "/user")
            .match_header("accept", "application/vnd.github+json")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = GitHubClient::new_with_base_url("ghp_t".into(), base).unwrap();
        let resp = client.request(reqwest::Method::GET, "/user").send().await.expect("send");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_adds_api_version_header() {
        let (mut server, base) = make_server().await;
        let m = server
            .mock("GET", "/user")
            .match_header("x-github-api-version", "2022-11-28")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = GitHubClient::new_with_base_url("ghp_t".into(), base).unwrap();
        let resp = client.request(reqwest::Method::GET, "/user").send().await.expect("send");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    #[tokio::test]
    async fn test_request_sets_user_agent_header() {
        let (mut server, base) = make_server().await;
        let m = server
            .mock("GET", "/user")
            .match_header("user-agent", "codespacectl/0.1 (https://github.com/topic-hash/codespacectl)")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = GitHubClient::new_with_base_url("ghp_t".into(), base).unwrap();
        let resp = client.request(reqwest::Method::GET, "/user").send().await.expect("send");
        assert_eq!(resp.status(), 200);
        m.assert_async().await;
    }

    // -------------------------------------------------------------------------
    // map_error() — uses mockito to construct a real `reqwest::Response`
    // against the configured base_url, then feeds it into `map_error`.
    // -------------------------------------------------------------------------

    /// Helper: spin up a mockito server that returns the given status + body
    /// for any path, then fetch the response and feed it to `map_error`.
    async fn map_error_for(
        status: usize,
        body: &str,
    ) -> std::result::Result<reqwest::Response, CodespaceError> {
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let _m = server
            .mock("GET", "/anything")
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let client = GitHubClient::new_with_base_url("ghp_test".into(), base).unwrap();
        let resp = client
            .request(reqwest::Method::GET, "/anything")
            .send()
            .await
            .expect("network request should complete");
        client.map_error(resp).await
    }

    #[tokio::test]
    async fn test_map_error_200_returns_ok() {
        let r = map_error_for(200, "{}").await;
        assert!(r.is_ok(), "200 should map to Ok");
    }

    #[tokio::test]
    async fn test_map_error_201_returns_ok() {
        let r = map_error_for(201, "{}").await;
        assert!(r.is_ok(), "201 should map to Ok");
    }

    #[tokio::test]
    async fn test_map_error_204_returns_ok() {
        let r = map_error_for(204, "").await;
        assert!(r.is_ok(), "204 should map to Ok");
    }

    #[tokio::test]
    async fn test_map_error_401_bad_credentials_returns_token_revoked() {
        let r = map_error_for(401, "{\"message\":\"Bad credentials\",\"status\":\"401\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::TokenRevoked)),
            "401 with 'Bad credentials' should map to TokenRevoked, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_401_invalid_token_returns_token_revoked() {
        let r = map_error_for(401, "{\"message\":\"invalid token\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::TokenRevoked)),
            "401 with 'invalid' in body should map to TokenRevoked, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_401_other_returns_auth_failed() {
        let r = map_error_for(401, "{\"message\":\"something else\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::AuthFailed(_))),
            "401 without 'Bad credentials'/'invalid' should map to AuthFailed, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_403_with_scope_returns_token_invalid_scope() {
        let r = map_error_for(403, "{\"message\":\"missing required scope\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::TokenInvalidScope { .. })),
            "403 with 'scope' in body should map to TokenInvalidScope, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_403_resource_not_accessible_returns_token_invalid_scope() {
        let r = map_error_for(403, "{\"message\":\"Resource not accessible by token\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::TokenInvalidScope { .. })),
            "403 with 'Resource not accessible' should map to TokenInvalidScope, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_403_other_returns_auth_failed() {
        let r = map_error_for(403, "{\"message\":\"forbidden for other reason\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::AuthFailed(_))),
            "403 without 'scope'/'Resource not accessible' should map to AuthFailed, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_404_returns_codespace_not_found() {
        let r = map_error_for(404, "{\"message\":\"Not Found\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::CodespaceNotFound(_))),
            "404 should map to CodespaceNotFound, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_429_returns_codespace_unreachable() {
        let r = map_error_for(429, "{\"message\":\"rate limit exceeded\"}").await;
        assert!(
            matches!(r, Err(CodespaceError::CodespaceUnreachable(_))),
            "429 should map to CodespaceUnreachable, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_500_returns_codespace_unreachable() {
        let r = map_error_for(500, "Internal Server Error").await;
        assert!(
            matches!(r, Err(CodespaceError::CodespaceUnreachable(_))),
            "500 should map to CodespaceUnreachable, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_503_returns_codespace_unreachable() {
        let r = map_error_for(503, "Service Unavailable").await;
        assert!(
            matches!(r, Err(CodespaceError::CodespaceUnreachable(_))),
            "503 should map to CodespaceUnreachable, got {:?}",
            r
        );
    }

    #[tokio::test]
    async fn test_map_error_418_returns_internal() {
        // 418 I'm a Teapot — not in any of the handled branches, so falls
        // through to the catch-all Internal arm.
        let r = map_error_for(418, "I'm a teapot").await;
        assert!(
            matches!(r, Err(CodespaceError::Internal(_))),
            "418 should map to Internal, got {:?}",
            r
        );
    }
}
