//! Integration tests — `codespacectl` help / version / smoke surface.
//!
//! These tests invoke the compiled `codespacectl` binary as a subprocess
//! via `assert_cmd` and exercise the clap-generated help and version output.
//! No network, no token, no state file required.

mod common;

use predicates::prelude::*;

use common::cargo_bin;

/// All 12 subcommands that should be listed in the top-level help. Used to
/// verify the help output mentions each one.
const ALL_SUBCOMMANDS: &[&str] = &[
    "init", "discover", "switch", "connect", "health", "exec", "raw", "stop",
    "state", "session", "doctor", "token",
];

/// `codespacectl --help` exits 0 and prints a banner containing "codespacectl".
#[test]
fn test_help_long_succeeds() {
    let mut cmd = cargo_bin();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("codespacectl"));
}

/// `codespacectl --help` mentions every subcommand in the surface.
#[test]
fn test_help_lists_all_subcommands() {
    let mut cmd = cargo_bin();
    cmd.arg("--help");
    let output = cmd.assert().success().get_output().stdout.clone();
    let help = String::from_utf8(output).expect("help output is utf8");
    for sub in ALL_SUBCOMMANDS {
        assert!(
            help.contains(sub),
            "expected top-level help to mention subcommand '{}' — full help:\n{}",
            sub,
            help
        );
    }
}

/// `codespacectl -h` is equivalent to `--help` (both succeed and mention
/// "codespacectl").
#[test]
fn test_help_short_matches_long() {
    let mut long_cmd = cargo_bin();
    long_cmd.arg("--help");
    let long_output = long_cmd
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let mut short_cmd = cargo_bin();
    short_cmd.arg("-h");
    let short_output = short_cmd
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let long = String::from_utf8(long_output).expect("long help utf8");
    let short = String::from_utf8(short_output).expect("short help utf8");
    // clap typically renders -h and --help identically (both use long help
    // when wrap_help is enabled). We assert the subcommand list matches.
    for sub in ALL_SUBCOMMANDS {
        assert!(short.contains(sub), "short help missing subcommand {}", sub);
        assert!(long.contains(sub), "long help missing subcommand {}", sub);
    }
}

/// `codespacectl --version` exits 0 and prints `codespacectl 0.1.0`.
#[test]
fn test_version_long() {
    let mut cmd = cargo_bin();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("codespacectl"))
        .stdout(predicate::str::contains("0.1.0"));
}

/// `codespacectl -V` is equivalent to `--version`.
#[test]
fn test_version_short() {
    let mut cmd = cargo_bin();
    cmd.arg("-V");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("codespacectl"))
        .stdout(predicate::str::contains("0.1.0"));
}

/// `codespacectl help` (the subcommand form) is equivalent to `--help`.
#[test]
fn test_help_subcommand() {
    let mut cmd = cargo_bin();
    cmd.arg("help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("codespacectl"));
}

/// `codespacectl help <subcommand>` succeeds for every subcommand in the
/// surface.
#[test]
fn test_help_per_subcommand() {
    for sub in ALL_SUBCOMMANDS {
        let mut cmd = cargo_bin();
        cmd.args(["help", sub]);
        let assert = cmd.assert().success();
        let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert!(
            out.contains(sub) || out.contains(&sub.replace('-', "_")),
            "expected `help {}` output to mention '{}' — got:\n{}",
            sub,
            sub,
            out
        );
    }
}

/// `codespacectl help nonexistent` exits non-zero (clap rejects unknown
/// subcommands in `help`).
#[test]
fn test_help_unknown_subcommand_fails() {
    let mut cmd = cargo_bin();
    cmd.args(["help", "nonexistent"]);
    cmd.assert().failure();
}

/// `codespacectl nonexistent-command` exits non-zero (unknown subcommand).
#[test]
fn test_unknown_command_fails() {
    let mut cmd = cargo_bin();
    cmd.arg("nonexistent-command");
    cmd.assert().failure();
}

/// `codespacectl` with no args exits non-zero (clap requires a subcommand
/// — `Cli.command` has no `Option` wrapper).
#[test]
fn test_no_args_fails() {
    let mut cmd = cargo_bin();
    cmd.assert().failure();
}

/// `codespacectl --json --help` still prints help (clap intercepts --help
/// before our dispatch logic ever runs, so --json is irrelevant here).
#[test]
fn test_json_help_still_prints_help() {
    let mut cmd = cargo_bin();
    cmd.args(["--json", "--help"]);
    // clap prints help to stdout and exits 0; --json has no effect.
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("codespacectl"));
}

/// `codespacectl --verbose` (with no subcommand) exits non-zero — clap
/// requires a subcommand even when global flags are present.
#[test]
fn test_verbose_without_subcommand_fails() {
    let mut cmd = cargo_bin();
    cmd.arg("--verbose");
    cmd.assert().failure();
}
