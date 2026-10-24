use std::process::Command;

fn doe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_doe"))
}

#[test]
fn version_flag_prints_doe_and_version() {
    let out = doe().arg("--version").output().expect("run doe");
    assert!(out.status.success(), "exit code: {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("doe "),
        "expected version line starting with 'doe ', got: {stdout:?}"
    );
}

#[test]
fn no_args_prints_usage_and_fails() {
    let out = doe().output().expect("run doe");
    assert!(
        !out.status.success(),
        "no args must not succeed, exit code: {}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Usage: doe"),
        "expected usage on stderr, got: {stderr:?}"
    );
}

#[test]
fn unknown_input_is_rejected_by_name() {
    let out = doe().arg("add").output().expect("run doe");
    assert!(
        !out.status.success(),
        "unknown input must not succeed, exit code: {}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("add"),
        "error must name the offending input, got: {stderr:?}"
    );
}
