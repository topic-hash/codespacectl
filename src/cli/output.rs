//! JSON envelope output for `--json` mode.
//!
//! Every command emits this envelope when `--json` is passed.

use crate::error::ErrorEnvelope;
use serde::{Deserialize, Serialize};

/// The full output envelope. Stable schema `codespacectl/v1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputEnvelope<T> {
    pub schema: String,
    pub ok: bool,
    pub result: Option<T>,
    pub error: Option<ErrorEnvelope>,
    pub warnings: Vec<String>,
    pub session: Option<SessionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRef {
    pub id: String,
    pub log_path: String,
}

impl<T> OutputEnvelope<T> {
    pub fn success(result: T) -> Self {
        Self {
            schema: "codespacectl/v1".to_string(),
            ok: true,
            result: Some(result),
            error: None,
            warnings: vec![],
            session: None,
        }
    }

    pub fn success_with_session(result: T, session: SessionRef) -> Self {
        Self {
            schema: "codespacectl/v1".to_string(),
            ok: true,
            result: Some(result),
            error: None,
            warnings: vec![],
            session: Some(session),
        }
    }

    pub fn failure(err: crate::CodespaceError) -> Self {
        let envelope: ErrorEnvelope = (&err).into();
        Self {
            schema: "codespacectl/v1".to_string(),
            ok: false,
            result: None,
            error: Some(envelope),
            warnings: vec![],
            session: None,
        }
    }
}

/// Print the envelope as JSON to stdout (for --json mode).
pub fn print_envelope<T: Serialize>(envelope: OutputEnvelope<T>) {
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).unwrap_or_else(|e| {
            serde_json::json!({
                "schema": "codespacectl/v1",
                "ok": false,
                "error": {
                    "kind": "internal_error",
                    "message": format!("failed to serialize envelope: {}", e),
                    "retryable": false,
                    "suggested_action": "report a bug"
                }
            })
            .to_string()
        })
    );
}
