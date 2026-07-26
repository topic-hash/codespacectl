//! HTTP client for GitHub API.

use crate::{CodespaceError, Result};
use reqwest::{Client, StatusCode};

const API_BASE: &str = "https://api.github.com";

pub struct GitHubClient {
    pub client: Client,
    pub token: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Result<Self> {
        let client = Client::builder()
            .user_agent("codespacectl/0.1 (https://github.com/topic-hash/codespacectl)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| CodespaceError::Internal(format!("reqwest client build failed: {}", e)))?;

        Ok(Self { client, token })
    }

    /// Build a request with auth headers.
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{}", API_BASE, path)
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
