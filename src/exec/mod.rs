//! Manifest command execution + lifecycle hooks.
//!
//! Wave 7 subagent: implement exec_command, run_post_start, run_pre_stop.

use crate::manifest::{Command, HookCommand, TemplateContext};
use crate::ssh::CodespaceSsh;
use crate::{CodespaceError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of an `exec` invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecOutput {
    pub command_name: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_secs: f64,
    pub session_id: String,
}

/// Execute a manifest-declared command via SSH.
pub async fn exec_command(
    ssh: &mut CodespaceSsh,
    command_name: &str,
    command: &Command,
    ctx: &TemplateContext,
    session_id: &str,
) -> Result<ExecOutput> {
    let _ = (ssh, command_name, command, ctx, session_id);
    // TODO (Wave 7 subagent): implement
    Err(CodespaceError::Internal(
        "exec_command not yet implemented — Wave 7 subagent pending".into(),
    ))
}

/// Run all postStart hooks.
pub async fn run_post_start(
    ssh: &mut CodespaceSsh,
    hooks: &[HookCommand],
    ctx: &TemplateContext,
) -> Result<()> {
    let _ = (ssh, hooks, ctx);
    // TODO (Wave 7 subagent): implement
    Err(CodespaceError::Internal(
        "run_post_start not yet implemented — Wave 7 subagent pending".into(),
    ))
}

/// Run all preStop hooks.
pub async fn run_pre_stop(
    ssh: &mut CodespaceSsh,
    hooks: &[HookCommand],
    ctx: &TemplateContext,
) -> Result<()> {
    let _ = (ssh, hooks, ctx);
    // TODO (Wave 7 subagent): implement
    Err(CodespaceError::Internal(
        "run_pre_stop not yet implemented — Wave 7 subagent pending".into(),
    ))
}

/// Execute an ad-hoc shell command (not declared in manifest).
pub async fn exec_raw(
    ssh: &mut CodespaceSsh,
    command: &str,
    timeout: Duration,
    session_id: &str,
) -> Result<ExecOutput> {
    let _ = (ssh, command, timeout, session_id);
    // TODO (Wave 7 subagent): implement
    Err(CodespaceError::Internal(
        "exec_raw not yet implemented — Wave 7 subagent pending".into(),
    ))
}
