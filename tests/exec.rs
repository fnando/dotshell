//! Runs the compiled `dotshell` binary with `--` to exec a command.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dotshell-test-{label}-{:?}-{}",
        std::thread::current().id(),
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_env(dir: &Path, name: &str, contents: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn exec_runs_command_with_loaded_env() {
    let dir = temp_dir("exec-loaded-env");
    let env_file = write_env(&dir, ".env", "GREETING=hello\n");

    let output = Command::new(env!("CARGO_BIN_EXE_dotshell"))
        .args([&env_file, "--", "printenv", "GREETING"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello\n");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn exec_last_file_wins_for_the_command() {
    let dir = temp_dir("exec-override");
    let base = write_env(&dir, ".env", "FOO=base\n");
    let overrides = write_env(&dir, ".env.local", "FOO=override\n");

    let output = Command::new(env!("CARGO_BIN_EXE_dotshell"))
        .args([&base, &overrides, "--", "printenv", "FOO"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "override\n");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn exec_does_not_leak_bookkeeping_vars() {
    let dir = temp_dir("exec-no-bookkeeping");
    let env_file = write_env(&dir, ".env", "FOO=1\n");

    let output = Command::new(env!("CARGO_BIN_EXE_dotshell"))
        .args([&env_file, "--", "printenv", "DOTSHELL_KEYS"])
        .output()
        .unwrap();

    // printenv exits non-zero when the variable is unset.
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn exec_passes_through_the_command_exit_status() {
    let dir = temp_dir("exec-exit-status");
    let env_file = write_env(&dir, ".env", "FOO=1\n");

    let output = Command::new(env!("CARGO_BIN_EXE_dotshell"))
        .args([&env_file, "--", "sh", "-c", "exit 42"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(42));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn exec_reports_missing_command_and_exits_127() {
    let dir = temp_dir("exec-missing-command");
    let env_file = write_env(&dir, ".env", "FOO=1\n");

    let output = Command::new(env!("CARGO_BIN_EXE_dotshell"))
        .args([&env_file, "--", "definitely-not-a-real-command-xyz"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to exec"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn exec_errors_on_malformed_env_file() {
    let dir = temp_dir("exec-malformed");
    let bad = write_env(&dir, ".env.bad", "this is not valid\n");

    let output = Command::new(env!("CARGO_BIN_EXE_dotshell"))
        .args([&bad, "--", "true"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("dotshell:"));
    fs::remove_dir_all(&dir).ok();
}
