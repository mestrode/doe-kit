//! End-to-end tests of the `doe-stats` binary: views, exit codes,
//! tiers and the section contracts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_doe-stats");

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doe-stats-cli-test-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture() -> String {
    fs::read_to_string("tests/fixtures/pool.json").unwrap()
}

/// Writes the fixture pool into `dir` under `name`.
fn write_fixture(dir: &Path, name: &str) -> (PathBuf, String) {
    let content = fixture();
    let file = dir.join(name);
    fs::write(&file, &content).unwrap();
    (file, content)
}

fn run(dir: &Path, args: &[&str]) -> (std::process::Output, String) {
    let output = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    (output, stdout)
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

// ---------------------------------------------------------------------
// bare invocation: the full report
// ---------------------------------------------------------------------

#[test]
fn bare_invocation_reports_all_three_sections() {
    let dir = temp_dir("bare");
    let (file, content) = write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    // the sections in order: input, sampling, output
    let input_at = stdout.find("input parameters:").expect("input section");
    let pool_at = stdout.find("pool: ").expect("pool line");
    let outputs_at = stdout.find("outputs:").expect("outputs section");
    assert!(
        input_at < pool_at && pool_at < outputs_at,
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("x: uniform, range [0, 10], scope [2, 8] (60.0% of range)"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("y: gaussian, range [0, 10], mean 5, sigma 2, scope: full range"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("pool: 4 designs, 4 evaluated, 2 parameters (2 designs per parameter)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    assert!(stdout.contains("a: n=4, min 1.000000,"), "stdout: {stdout}");
    assert!(stdout.contains("b: n=4, min 2.000000,"), "stdout: {stdout}");
    // b = 2a: the single output pair correlates perfectly
    assert!(
        stdout.contains("output pairs: 1, max |pearson| 1.000000, max |spearman| 1.000000"),
        "stdout: {stdout}"
    );
    // the compact tier keeps the details behind --verbose
    assert!(!stdout.contains("decile counts:"), "stdout: {stdout}");
    assert!(!stdout.contains("  a vs b: pearson"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn bare_invocation_verbose_lists_all_details() {
    let dir = temp_dir("bare-verbose");
    write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["pool.json", "--verbose"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(stdout.contains("decile counts: ["), "stdout: {stdout}");
    assert!(stdout.contains("x vs y: pearson"), "stdout: {stdout}");
    assert!(stdout.contains("in scope: "), "stdout: {stdout}");
    assert!(
        stdout.contains("  a vs b: pearson 1.000000, spearman 1.000000 (n 4)"),
        "stdout: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn bare_invocation_json_reports_all_sections() {
    let dir = temp_dir("bare-json");
    write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["pool.json", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "all");
    assert_eq!(v["file"], "pool.json");
    assert_eq!(v["evaluated"], 4);
    assert_eq!(v["input"]["parameters"].as_array().unwrap().len(), 2);
    assert_eq!(v["pool"]["total"], 4);
    assert_eq!(v["pool"]["n_pairs"], 1);
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
    assert_eq!(v["output_pairs"]["n_pairs"], 1);
    assert_eq!(v["output_pairs"]["max_abs_pearson"], 1.0);
    // compact tier: no detail arrays
    assert!(v["pool"].get("parameters").is_none());
    assert!(v["output_pairs"].get("pairs").is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn bare_invocation_without_results_exits_1() {
    let dir = temp_dir("bare-noresults");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // the full report stops at the first failing section: with designs
    // but without results that is the output section
    let (out, _) = run(&dir, &["pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr_of(&out).contains("no results to analyze"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn bare_invocation_without_designs_exits_1() {
    let dir = temp_dir("bare-nodesigns");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["designs"] = Value::Array(vec![]);
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // a fresh pool fails at the sampling section
    let (out, _) = run(&dir, &["pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr_of(&out).contains("no designs to analyze"));
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------
// input view
// ---------------------------------------------------------------------

#[test]
fn input_reports_the_definitions_only() {
    let dir = temp_dir("input");
    let (file, content) = write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["input", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(
        stdout.starts_with("input parameters:\n"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("x: uniform, range [0, 10], scope [2, 8] (60.0% of range)"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("y: gaussian, range [0, 10], mean 5, sigma 2, scope: full range"),
        "stdout: {stdout}"
    );
    // the definitions only: no pool line, no outputs
    assert!(!stdout.contains("pool: "), "stdout: {stdout}");
    assert!(!stdout.contains("outputs:"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn input_works_without_designs_and_results() {
    let dir = temp_dir("input-fresh");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["designs"] = Value::Array(vec![]);
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // definitions need neither designs nor results: a fresh pool works
    let (out, stdout) = run(&dir, &["input", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(stdout.contains("input parameters:"), "stdout: {stdout}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn input_ignores_unknown_result_names() {
    let dir = temp_dir("input-unknown");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"][3]["name"] = Value::String("9.9".to_owned());
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // result names are validated only where results are read: the
    // input view never reads them
    let (out, _) = run(&dir, &["input", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn input_json_reports_the_definitions() {
    let dir = temp_dir("input-json");
    write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["input", "--json", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "input");
    assert_eq!(v["file"], "pool.json");
    let params = v["input"]["parameters"].as_array().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["name"], "x");
    assert_eq!(params[0]["distribution"], "uniform");
    assert_eq!(params[0]["scope"], serde_json::json!([2.0, 8.0]));
    assert_eq!(params[0]["scope_fraction"], 0.6);
    assert_eq!(params[1]["distribution"], "gaussian");
    assert_eq!(params[1]["mean"], 5.0);
    assert_eq!(params[1]["sigma"], 2.0);
    assert!(params[1].get("scope").is_none());
    assert_eq!(params[1]["scope_fraction"], 1.0);
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------
// sampling view
// ---------------------------------------------------------------------

#[test]
fn sampling_reports_the_sampling_points_only() {
    let dir = temp_dir("sampling");
    let (file, content) = write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["sampling", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(
        stdout.contains("pool: 4 designs, 4 evaluated,"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    // the sampling points only: no definitions, no outputs
    assert!(!stdout.contains("input parameters:"), "stdout: {stdout}");
    assert!(!stdout.contains("outputs:"), "stdout: {stdout}");
    assert!(!stdout.contains("output pairs:"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn sampling_without_designs_exits_1() {
    let dir = temp_dir("sampling-nodesigns");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["designs"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, _) = run(&dir, &["sampling", "pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr_of(&out).contains("no designs to analyze"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn sampling_ignores_unknown_result_names() {
    let dir = temp_dir("sampling-unknown");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"][3]["name"] = Value::String("9.9".to_owned());
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // the sampling view counts the result entries but never reads
    // their names against the designs
    let (out, stdout) = run(&dir, &["sampling", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(stdout.contains("4 evaluated"), "stdout: {stdout}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn sampling_json_reports_the_pool_statistics() {
    let dir = temp_dir("sampling-json");
    write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["sampling", "--json", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "sampling");
    assert_eq!(v["file"], "pool.json");
    assert_eq!(v["evaluated"], 4);
    assert_eq!(v["pool"]["total"], 4);
    assert_eq!(v["pool"]["n_pairs"], 1);
    assert!(v.get("input").is_none());
    assert!(v.get("results").is_none());
    assert!(v.get("output_pairs").is_none());
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------
// output view
// ---------------------------------------------------------------------

#[test]
fn output_reports_results_and_output_pairs() {
    let dir = temp_dir("output");
    let (file, content) = write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["output", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(stdout.starts_with("outputs:\n"), "stdout: {stdout}");
    assert!(stdout.contains("a: n=4, min 1.000000,"), "stdout: {stdout}");
    assert!(stdout.contains("b: n=4, min 2.000000,"), "stdout: {stdout}");
    // b = 2a: one pair, perfectly correlated
    assert!(
        stdout.contains("output pairs: 1, max |pearson| 1.000000, max |spearman| 1.000000"),
        "stdout: {stdout}"
    );
    // compact tier: the per-parameter blocks and the pair list are
    // behind --verbose
    assert!(!stdout.contains("in scope:"), "stdout: {stdout}");
    assert!(!stdout.contains("  a vs b: pearson"), "stdout: {stdout}");
    // the outputs only: no definitions, no sampling points
    assert!(!stdout.contains("input parameters:"), "stdout: {stdout}");
    assert!(!stdout.contains("sampling points:"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_verbose_lists_the_pair_details() {
    let dir = temp_dir("output-verbose");
    write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["output", "--verbose", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(stdout.contains("in scope: "), "stdout: {stdout}");
    assert!(stdout.contains("deciles: ["), "stdout: {stdout}");
    assert!(
        stdout.contains("  a vs b: pearson 1.000000, spearman 1.000000 (n 4)"),
        "stdout: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_with_a_single_variable_omits_the_pairs() {
    let dir = temp_dir("output-single");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    for entry in root["results"].as_array_mut().unwrap() {
        entry
            .as_object_mut()
            .unwrap()
            .remove("b")
            .expect("the entry carries b");
    }
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // with fewer than two result variables no pair exists: the block
    // is omitted (not an error)
    let (out, stdout) = run(&dir, &["output", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(stdout.contains("outputs:"), "stdout: {stdout}");
    assert!(stdout.contains("a: n=4,"), "stdout: {stdout}");
    assert!(!stdout.contains("output pairs:"), "stdout: {stdout}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_without_results_exits_1() {
    let dir = temp_dir("output-noresults");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, _) = run(&dir, &["output", "pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr_of(&out).contains("no results to analyze"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_unknown_result_name_exits_1() {
    let dir = temp_dir("output-unknown");
    let file = dir.join("pool.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"][3]["name"] = Value::String("9.9".to_owned());
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // the output view reads the results: names are validated here
    let (out, _) = run(&dir, &["output", "pool.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr_of(&out).contains("does not match any design"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn output_json_reports_results_and_pairs() {
    let dir = temp_dir("output-json");
    write_fixture(&dir, "pool.json");

    let (out, stdout) = run(&dir, &["output", "--json", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "output");
    assert_eq!(v["evaluated"], 4);
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
    assert_eq!(v["output_pairs"]["n_pairs"], 1);
    assert_eq!(v["output_pairs"]["max_abs_pearson"], 1.0);
    assert!(v["output_pairs"].get("pairs").is_none());
    assert!(v["results"][0].get("parameters").is_none());
    assert!(v.get("input").is_none());
    assert!(v.get("pool").is_none());

    // --verbose keeps the full pair list and per-parameter blocks
    let (out, stdout) = run(&dir, &["output", "--json", "--verbose", "pool.json"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    let pairs = v["output_pairs"]["pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0]["a"], "a");
    assert_eq!(pairs[0]["b"], "b");
    assert_eq!(pairs[0]["n"], 4);
    assert_eq!(pairs[0]["pearson"], 1.0);
    assert_eq!(pairs[0]["spearman"], 1.0);
    assert_eq!(v["results"][0]["parameters"].as_array().unwrap().len(), 2);
    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------
// CLI contract
// ---------------------------------------------------------------------

#[test]
fn unknown_subcommand_is_a_usage_error() {
    let dir = temp_dir("unknown");
    write_fixture(&dir, "pool.json");

    let (out, _) = run(&dir, &["nonsense", "pool.json"]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_file_and_no_subcommand_is_a_usage_error() {
    let dir = temp_dir("noargs");

    let (out, _) = run(&dir, &[]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_file_is_an_io_error() {
    let dir = temp_dir("missing");

    let (out, _) = run(&dir, &["input", "absent.json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!dir.join("absent.json").exists());
    fs::remove_dir_all(&dir).ok();
}
