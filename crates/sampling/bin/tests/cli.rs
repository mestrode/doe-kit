//! End-to-end tests of the `sampling` binary: exit codes, backup, file writing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_sampling");

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sampling-cli-test-{}-{tag}", std::process::id()));
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

    let (out, stdout) = run(&dir, &["plhs", "--add", "2", "designs.json"]);
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

    let (out, stdout) = run(&dir, &["plhs", "--add", "2", "designs.json"]);
    assert!(out.status.success());
    assert!(stdout.contains("iteration 1"), "first run: {stdout}");

    let (out, stdout) = run(&dir, &["plhs", "--add", "1", "designs.json"]);
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
            "plhs",
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
    let (out, _) = run(&dir, &["plhs", "--add", "2", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));

    fs::write(&file, fixture()).unwrap();
    let (out, _) = run(&dir, &["plhs", "--add", "0", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));

    // scope missing an entry for a declared parameter
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["scope"].as_array_mut().unwrap().pop();
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();
    let (out, _) = run(&dir, &["plhs", "--add", "2", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("scope"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn explore_mode_covers_the_whole_surface() {
    let dir = temp_dir("explore");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["plhs", "--explore", "2", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("explored 2 designs (iteration 1), score "),
        "unexpected summary: {stdout}"
    );

    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let new = root["new_designs"].as_array().unwrap();
    assert_eq!(new.len(), 2);
    assert_eq!(new[0]["name"], "1.1");
    assert_eq!(new[1]["name"], "1.2");

    // the new designs cover the full definition range [0, 10] of both
    // parameters, ignoring the scopes [2, 8] and [3, 7]
    let mut outside_scope = false;
    for design in new {
        for (p, lo, hi) in [("x", 0.0, 10.0), ("y", 0.0, 10.0)] {
            let v = design[p].as_f64().unwrap();
            assert!((lo..=hi).contains(&v), "{p}={v} outside [0, 10]");
        }
        let x = design["x"].as_f64().unwrap();
        let y = design["y"].as_f64().unwrap();
        if !(2.0..=8.0).contains(&x) || !(3.0..=7.0).contains(&y) {
            outside_scope = true;
        }
    }
    assert!(
        outside_scope,
        "explore produced only in-scope values; the scopes were not ignored"
    );

    // with the same seed, explore produces values that differ from the
    // scoped --add run: the mode genuinely changes the outcome
    let add_file = dir.join("add.json");
    fs::write(&add_file, fixture()).unwrap();
    let (out, _) = run(&dir, &["plhs", "--add", "2", add_file.to_str().unwrap()]);
    assert!(out.status.success());
    let add_root: Value = serde_json::from_str(&fs::read_to_string(&add_file).unwrap()).unwrap();
    let differs = new
        .iter()
        .zip(add_root["new_designs"].as_array().unwrap())
        .any(|(e, a)| e["x"] != a["x"] || e["y"] != a["y"]);
    assert!(differs, "explore and add produced identical designs");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn explore_and_add_are_mutually_exclusive() {
    let dir = temp_dir("exclusive");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    // both modes given
    let (out, _) = run(
        &dir,
        &["plhs", "--add", "2", "--explore", "1", "designs.json"],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("mutually exclusive"), "stderr: {stderr}");

    // neither mode given
    let (out, _) = run(&dir, &["plhs", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("one of --add or --explore is required"),
        "stderr: {stderr}"
    );

    // zero designs in explore mode
    let (out, _) = run(&dir, &["plhs", "--explore", "0", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--explore"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_file_exits_2() {
    let dir = temp_dir("missing");
    let (out, _) = run(&dir, &["plhs", "--add", "2", "does-not-exist.json"]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn subcommand_is_required_and_validated() {
    let dir = temp_dir("subcommand");

    // no subcommand: usage error
    let (out, _) = run(&dir, &[]);
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("subcommand"), "stderr: {stderr}");

    // unknown subcommand
    let (out, _) = run(&dir, &["nope"]);
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nope"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_reports_variables_and_coverage() {
    let dir = temp_dir("stats");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["stats", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("pool: 30 designs, 3 evaluated"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("energy: n=3,"), "stdout: {stdout}");
    assert!(stdout.contains("pearson"), "stdout: {stdout}");
    assert!(stdout.contains("deciles"), "stdout: {stdout}");
    // the sampling points themselves are reported too, alongside the
    // result variables
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    assert!(stdout.contains("x vs y: pearson "), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), fixture());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_without_results_reports_sampling_points() {
    let dir = temp_dir("stats-empty");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"] = Value::Array(vec![]);
    let content = serde_json::to_string(&root).unwrap();
    fs::write(&file, &content).unwrap();

    // designs exist, nothing is evaluated: the sampling points themselves
    // are analyzed instead of erroring out
    let (out, stdout) = run(&dir, &["stats", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("pool: 30 designs, 0 evaluated"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    assert!(stdout.contains("x: n=30, min "), "stdout: {stdout}");
    assert!(stdout.contains("y: n=30, min "), "stdout: {stdout}");
    assert!(stdout.contains("decile counts: ["), "stdout: {stdout}");
    assert!(stdout.contains("x vs y: pearson "), "stdout: {stdout}");
    // no result variable exists, so none is reported
    assert!(!stdout.contains("energy"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_without_designs_exits_1() {
    let dir = temp_dir("stats-nodesigns");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["designs"] = Value::Array(vec![]);
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // a pool straight from `init` without any exploration: nothing to
    // analyze at all
    let (out, _) = run(&dir, &["stats", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no designs to analyze"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_unknown_result_name_exits_1() {
    let dir = temp_dir("stats-unknown");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"][0]["name"] = Value::String("99.1".to_owned());
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, _) = run(&dir, &["stats", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does not match any design"),
        "stderr: {stderr}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn init_refuses_an_existing_file() {
    let dir = temp_dir("init-exists");
    let file = dir.join("designs.json");
    fs::write(&file, "sentinel").unwrap();

    let (out, _) = run(&dir, &["init", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error: 'designs.json' already exists"),
        "stderr: {stderr}"
    );

    // the existing file is left untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), "sentinel");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_ignores_non_numeric_result_fields() {
    let dir = temp_dir("stats-nonnumeric");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"][0]["comment"] = Value::String("ok".to_owned());
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, stdout) = run(&dir, &["stats", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stdout.contains("comment"), "stdout: {stdout}");
    assert!(stdout.contains("energy"), "stdout: {stdout}");
    fs::remove_dir_all(&dir).ok();
}
