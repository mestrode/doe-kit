//! End-to-end tests of the `simio` binary: exit codes, backup, skipping,
//! file writing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_simio");

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("simio-cli-test-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small pool: params a, b, c and two designs (matching the init
/// parameter names, but fixed values).
fn pool() -> String {
    serde_json::json!({
        "params": [
            { "name": "a", "min": 0, "max": 10, "distribution": "uniform" },
            { "name": "b", "min": -1, "max": 1, "distribution": "uniform" },
            { "name": "c", "min": 0, "max": 5, "mean": 2.5, "sigma": 1,
              "distribution": "gaussian" }
        ],
        "designs": [
            { "name": "1.1", "a": 5, "b": 0, "c": 2.5 },
            { "name": "1.2", "a": 2.5, "b": -1, "c": 0 }
        ]
    })
    .to_string()
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
fn test_evaluates_every_fresh_design_and_writes_the_backup() {
    let dir = temp_dir("fresh");
    let file = dir.join("pool.json");
    fs::write(&file, pool()).unwrap();
    let original = fs::read(&file).unwrap();

    let (out, stdout) = run(&dir, &["test", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("evaluated 2 designs, results: linear, bowl, wave"),
        "unexpected summary: {stdout}"
    );

    // backup equals the untouched input
    assert_eq!(fs::read(dir.join("pool.json.bak")).unwrap(), original);

    // each design has one results entry with the three variables
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let results = root["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["name"], "1.1");
    for key in ["bowl", "linear", "wave"] {
        assert!(results[0][key].is_f64(), "missing result variable '{key}'");
    }
    // design 1.1 is the pool center: bowl's global minimum
    assert_eq!(results[0]["bowl"], 0.0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_skips_solved_designs_and_writes_nothing() {
    let dir = temp_dir("skip");
    let file = dir.join("pool.json");
    fs::write(&file, pool()).unwrap();

    let (out, _) = run(&dir, &["test", "pool.json"]);
    assert!(out.status.success());
    let before = fs::read(&file).unwrap();

    // second run: everything is solved, so nothing is written
    let (out, stdout) = run(&dir, &["test", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("evaluated 0 designs (2 already solved, skipped)"),
        "unexpected summary: {stdout}"
    );
    assert_eq!(fs::read(&file).unwrap(), before, "file must stay untouched");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_partial_results_evaluate_the_rest() {
    let dir = temp_dir("partial");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&pool()).unwrap();
    root["results"] = serde_json::json!([{ "name": "1.1", "bowl": 0.0 }]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, stdout) = run(&dir, &["test", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("evaluated 1 designs (1 already solved, skipped)"),
        "unexpected summary: {stdout}"
    );
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let results = root["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[1]["name"], "1.2");
    // the pre-existing entry is untouched
    assert_eq!(results[0]["bowl"], 0.0);
    assert!(results[0].get("linear").is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_missing_param_exits_1_and_names_it() {
    let dir = temp_dir("missing");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&pool()).unwrap();
    root["params"].as_array_mut().unwrap().remove(1); // drop b
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, stdout) = run(&dir, &["test", "pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout.is_empty(), "stdout on error: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing parameter 'b'"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_extra_params_are_allowed_and_ignored() {
    let dir = temp_dir("extra");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&pool()).unwrap();
    root["params"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "name": "d", "min": 0, "max": 1 }));
    for design in root["designs"].as_array_mut().unwrap() {
        design["d"] = serde_json::json!(0.5);
    }
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, stdout) = run(&dir, &["test", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("evaluated 2 designs"),
        "unexpected summary: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_no_backup_flag_skips_the_backup() {
    let dir = temp_dir("nobackup");
    let file = dir.join("pool.json");
    fs::write(&file, pool()).unwrap();

    let (out, _) = run(&dir, &["test", "--no-backup", "pool.json"]);
    assert!(out.status.success());
    assert!(!dir.join("pool.json.bak").exists(), "no backup expected");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_json_prints_one_machine_readable_line() {
    let dir = temp_dir("json");
    let file = dir.join("pool.json");
    fs::write(&file, pool()).unwrap();

    let (out, stdout) = run(&dir, &["test", "--json", "pool.json"]);
    assert!(out.status.success());
    let v: Value = serde_json::from_str(stdout.trim_end()).expect("one JSON line: {stdout}");
    assert_eq!(v["subcommand"], "test");
    assert_eq!(v["designs"], 2);
    assert_eq!(v["evaluated"], 2);
    assert_eq!(v["skipped"], 0);
    assert_eq!(
        v["variables"],
        serde_json::json!(["linear", "bowl", "wave"])
    );

    // errors stay human on stderr, stdout stays empty, exit 1
    let mut root: Value = serde_json::from_str(&pool()).unwrap();
    root["params"].as_array_mut().unwrap().remove(1);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();
    let (out, stdout) = run(&dir, &["test", "--json", "pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout.is_empty(), "stdout on error: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing parameter 'b'"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}
