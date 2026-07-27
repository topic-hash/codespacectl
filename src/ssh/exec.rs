//! SSH exec command runner.
//!
//! TODO (Wave 4 subagent): implement via russh channel.

use crate::ssh::transport::ExecResult as SshExecResult;
use crate::ssh::transport::SshError;

pub type ExecResult = SshExecResult;
pub type ExecError = SshError;
