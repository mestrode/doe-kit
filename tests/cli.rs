//! End-to-end tests of the `slhs` binary: exit codes, backup, file writing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_slhs");

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slhs-cli-test-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture() -> String {
    fs::read_to_string("tests/fixtures/input.json").unwrap()
}

fn run(dir: &PathBuf, args: &[&str]) -> (std::process::Output, String) {
    let output = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    (output, stdout)
}

#[test]
fn happy_path_writes_output_and_backup() {
    let dir = temp_dir("happy");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();
    let original = fs::read(&file).unwrap();

    let (out, stdout) = run(&dir, &["--add", "2", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("added 2 designs (iteration 1), score "),
        "unexpected summary: {stdout}"
    );

    // backup equals the original input
    let backup = fs::read(dir.join("designs.json.bak")).unwrap();
    assert_eq!(backup, original);

    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(root["designs"].as_array().unwrap().len(), 32);
    let new = root["new_designs"].as_array().unwrap();
    assert_eq!(new.len(), 2);
    assert_eq!(new[0]["name"], "1.1");
    assert_eq!(new[1]["name"], "1.2");
    // results pass through untouched
    assert_eq!(root["results"].as_array().unwrap().len(), 3);
    // unknown top-level fields survive
    assert!(root.get("params").is_some());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn second_run_increments_iteration_and_overwrites_new_designs() {
    let dir = temp_dir("second");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["--add", "2", "designs.json"]);
    assert!(out.status.success());
    assert!(stdout.contains("iteration 1"), "first run: {stdout}");

    let (out, stdout) = run(&dir, &["--add", "1", "designs.json"]);
    assert!(out.status.success());
    assert!(stdout.contains("iteration 2"), "second run: {stdout}");

    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let designs = root["designs"].as_array().unwrap();
    assert_eq!(designs.len(), 33); // 30 + 2 + 1
    let new = root["new_designs"].as_array().unwrap();
    assert_eq!(new.len(), 1);
    assert_eq!(new[0]["name"], "2.1");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_option_and_no_backup() {
    let dir = temp_dir("output");
    let file = dir.join("in.json");
    fs::write(&file, fixture()).unwrap();
    let original = fs::read(&file).unwrap();

    let (out, _) = run(
        &dir,
        &[
            "--add",
            "2",
            "--no-backup",
            "--output",
            "out.json",
            "in.json",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // input untouched, no backup, output written
    assert_eq!(fs::read(&file).unwrap(), original);
    assert!(!dir.join("in.json.bak").exists());
    let root: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("out.json")).unwrap()).unwrap();
    assert_eq!(root["new_designs"].as_array().unwrap().len(), 2);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn validation_errors_exit_1() {
    let dir = temp_dir("invalid");
    let file = dir.join("bad.json");
    fs::write(&file, "{ not json").unwrap();
    let (out, _) = run(&dir, &["--add", "2", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));

    fs::write(&file, fixture()).unwrap();
    let (out, _) = run(&dir, &["--add", "0", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));

    // scope missing an entry for a declared parameter
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["scope"].as_array_mut().unwrap().pop();
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();
    let (out, _) = run(&dir, &["--add", "2", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("scope"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_file_exits_2() {
    let dir = temp_dir("missing");
    let (out, _) = run(&dir, &["--add", "2", "does-not-exist.json"]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_dir_all(&dir).ok();
}
