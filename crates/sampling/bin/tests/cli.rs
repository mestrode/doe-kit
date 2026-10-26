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
fn plhs_json_emits_one_machine_readable_line() {
    let dir = temp_dir("plhs-json");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["plhs", "--add", "2", "--json", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // one line of JSON only: the human summary is suppressed
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "plhs");
    assert_eq!(v["mode"], "add");
    assert_eq!(v["file"], "designs.json");
    assert_eq!(v["seed"], 42);
    assert_eq!(v["cap"], 1_000_000);
    assert_eq!(v["m"], 2);
    assert_eq!(v["designs_before"], 30);
    assert_eq!(v["designs_after"], 32);
    assert_eq!(v["iteration"], 1);
    assert_eq!(v["n_params"], 2);
    // only a handful of assignments are possible on the fixture: the
    // exact enumeration ran
    assert_eq!(v["strategy"], "exact");
    // a single-chunk exact run is jointly optimal over the polish swap
    // space: the pass is skipped and reports as much
    assert_eq!(v["polished"], false);
    assert!(v.get("polish_moves").is_none());
    assert!(v.get("polish_score_before").is_none());
    assert!(v["score"].as_f64().unwrap() >= 0.0);
    assert!(v["elapsed_ms"].as_u64().is_some(), "elapsed_ms in {stdout}");
    // the pool is written and backed up exactly as without --json
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(root["new_designs"].as_array().unwrap().len(), 2);
    assert!(dir.join("designs.json.bak").exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn plhs_json_reports_greedy_with_cap_zero() {
    let dir = temp_dir("plhs-json-greedy");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(
        &dir,
        &["plhs", "--add", "2", "--cap", "0", "--json", "designs.json"],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["cap"], 0);
    assert_eq!(v["strategy"], "greedy");
    // a greedy run is polished by default and reports what the pass did
    assert_eq!(v["polished"], true);
    assert!(
        v["polish_moves"].as_u64().is_some(),
        "polish_moves in {stdout}"
    );
    assert!(v["polish_score_before"].as_f64().is_some());
    assert!(v["polish_score_after"].as_f64().is_some());
    assert!(v["score"].as_f64().unwrap() >= 0.0);
    assert!(v["elapsed_ms"].as_u64().is_some(), "elapsed_ms in {stdout}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn plhs_json_keeps_validation_errors_human() {
    let dir = temp_dir("plhs-json-invalid");
    let file = dir.join("bad.json");
    fs::write(&file, "{ not json").unwrap();

    // errors stay human on stderr and print no JSON: --json only
    // changes the success output
    let (out, stdout) = run(&dir, &["plhs", "--add", "2", "--json", "bad.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout.is_empty(), "stdout on error: {stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error"), "stderr: {stderr}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn polish_refines_the_first_exploration_and_reports_itself() {
    let dir = temp_dir("polish");

    // a fresh pool, first exploration: the greedy chunk on an empty
    // pool drifts, and the quality stage is the polish pass
    let (out, _) = run(&dir, &["init", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (out, stdout) = run(&dir, &["plhs", "--explore", "10", "--json", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["strategy"], "greedy");
    assert_eq!(v["polished"], true);
    let moves = v["polish_moves"].as_u64().expect("polish_moves");
    assert!(
        moves > 0,
        "the drifted assignment must have improving swaps: {stdout}"
    );
    let before = v["polish_score_before"].as_f64().unwrap();
    let after = v["polish_score_after"].as_f64().unwrap();
    assert!(after < before, "polish must improve: {before} -> {after}");
    assert_eq!(v["score"].as_f64().unwrap(), after);

    // the human summary reports the pass too; a second fresh pool, so
    // the wording is checked independently of the pool state
    let (out, _) = run(&dir, &["init", "plain.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (out, stdout) = run(&dir, &["plhs", "--explore", "10", "plain.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains(&format!(", polished with {moves} swap(s) (was ")),
        "unexpected summary: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_polish_flag_disables_the_pass() {
    let dir = temp_dir("no-polish");

    let (out, _) = run(&dir, &["init", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (out, stdout) = run(
        &dir,
        &[
            "plhs",
            "--explore",
            "10",
            "--no-polish",
            "--json",
            "pool.json",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["strategy"], "greedy");
    assert_eq!(v["polished"], false);
    assert!(v.get("polish_moves").is_none());
    assert!(v.get("polish_score_before").is_none());
    assert!(v.get("polish_score_after").is_none());
    // the run still writes the pool and backs it up
    assert!(dir.join("pool.json.bak").exists());
    let root: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pool.json")).unwrap()).unwrap();
    assert_eq!(root["designs"].as_array().unwrap().len(), 10);
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
        stdout.contains("pool: 30 designs, 3 evaluated, 2 parameters (15 designs per parameter)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("energy: n=3,"), "stdout: {stdout}");
    assert!(stdout.contains("pearson"), "stdout: {stdout}");
    assert!(stdout.contains("deciles"), "stdout: {stdout}");
    // the sampling points themselves are reported too, alongside the
    // result variables
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    assert!(stdout.contains("x vs y: pearson "), "stdout: {stdout}");
    assert!(stdout.contains("outer deciles: "), "stdout: {stdout}");
    assert!(
        stdout.contains("min pairwise distance: "),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("mean nearest neighbor: "),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("discrepancy (CL2): "), "stdout: {stdout}");

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
        stdout.contains("pool: 30 designs, 0 evaluated, 2 parameters (15 designs per parameter)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    assert!(stdout.contains("x: n=30, min "), "stdout: {stdout}");
    assert!(stdout.contains("y: n=30, min "), "stdout: {stdout}");
    assert!(stdout.contains("decile counts: ["), "stdout: {stdout}");
    assert!(stdout.contains("outer deciles: "), "stdout: {stdout}");
    assert!(
        stdout.contains("min pairwise distance: "),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("mean nearest neighbor: "),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("discrepancy (CL2): "), "stdout: {stdout}");
    assert!(stdout.contains("x vs y: pearson "), "stdout: {stdout}");
    // no result variable exists, so none is reported
    assert!(!stdout.contains("energy"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), content);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_single_design_reports_na_distances() {
    let dir = temp_dir("stats-single");
    let file = dir.join("designs.json");
    fs::write(
        &file,
        concat!(
            "{\n",
            "  \"params\": [{ \"name\": \"x\", \"min\": 0, \"max\": 10, \"distribution\": \"uniform\" }],\n",
            "  \"designs\": [{ \"name\": \"0.1\", \"x\": 5 }]\n",
            "}\n"
        ),
    )
    .unwrap();

    // one design: no pair exists, so the distances are n/a, but the
    // discrepancy of a single centered point is well defined (sqrt(1/12))
    let (out, stdout) = run(&dir, &["stats", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("pool: 1 designs, 0 evaluated, 1 parameters (1 designs per parameter)"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("min pairwise distance: n/a"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("mean nearest neighbor: n/a"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("discrepancy (CL2): 0.288675"),
        "stdout: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_fractional_designs_per_parameter() {
    let dir = temp_dir("stats-fraction");
    let file = dir.join("designs.json");
    fs::write(
        &file,
        concat!(
            "{\n",
            "  \"params\": [\n",
            "    { \"name\": \"x\", \"min\": 0, \"max\": 10, \"distribution\": \"uniform\" },\n",
            "    { \"name\": \"y\", \"min\": 0, \"max\": 10, \"distribution\": \"uniform\" }\n",
            "  ],\n",
            "  \"designs\": [\n",
            "    { \"name\": \"0.1\", \"x\": 1, \"y\": 2 },\n",
            "    { \"name\": \"0.2\", \"x\": 4, \"y\": 5 },\n",
            "    { \"name\": \"0.3\", \"x\": 7, \"y\": 8 }\n",
            "  ]\n",
            "}\n"
        ),
    )
    .unwrap();

    // 3 designs over 2 parameters: the context number is fractional
    let (out, stdout) = run(&dir, &["stats", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("pool: 3 designs, 0 evaluated, 2 parameters (1.5 designs per parameter)"),
        "stdout: {stdout}"
    );
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
fn explore_works_on_a_fresh_init_pool() {
    let dir = temp_dir("init-explore");

    // create a fresh pool, then run the first exploration on it
    let (out, _) = run(&dir, &["init", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (out, stdout) = run(&dir, &["plhs", "--explore", "10", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("explored 10 designs (iteration 1), score "),
        "unexpected summary: {stdout}"
    );

    let root: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pool.json")).unwrap()).unwrap();
    let designs = root["designs"].as_array().unwrap();
    assert_eq!(designs.len(), 10);
    assert_eq!(designs[0]["name"], "1.1");
    assert_eq!(designs[9]["name"], "1.10");
    let new = root["new_designs"].as_array().unwrap();
    assert_eq!(new.len(), 10);

    // the backup holds the untouched init skeleton
    let backup: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pool.json.bak")).unwrap()).unwrap();
    assert_eq!(backup["designs"].as_array().unwrap().len(), 0);
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

#[test]
fn stats_json_reports_the_full_pool_analysis() {
    let dir = temp_dir("stats-json");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["stats", "--json", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // one line of JSON only: the human report is suppressed
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "stats");
    assert_eq!(v["file"], "designs.json");
    assert_eq!(v["evaluated"], 3);
    let pool = &v["pool"];
    assert_eq!(pool["total"], 30);
    let params = pool["parameters"].as_array().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["name"], "x");
    assert_eq!(params[0]["n"], 30);
    assert_eq!(params[0]["deciles"].as_array().unwrap().len(), 10);
    assert!(params[0]["outer_deciles"].as_u64().is_some());
    assert!(pool["min_pairwise_distance"].is_number());
    assert!(pool["mean_nearest_neighbor"].is_number());
    assert!(pool["discrepancy_cl2"].is_number());
    let pairs = pool["pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0]["a"], "x");
    assert_eq!(pairs[0]["b"], "y");
    assert!(pairs[0]["pearson"].is_number());
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "energy");
    assert_eq!(results[0]["n"], 3);
    assert_eq!(results[0]["total"], 30);
    assert_eq!(results[0]["parameters"].as_array().unwrap().len(), 2);

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), fixture());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_json_without_results_has_empty_results_array() {
    let dir = temp_dir("stats-json-empty");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    let (out, stdout) = run(&dir, &["stats", "--json", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "stats");
    assert_eq!(v["evaluated"], 0);
    assert_eq!(
        v["results"].as_array().unwrap().len(),
        0,
        "stdout: {stdout}"
    );
    assert_eq!(v["pool"]["total"], 30);
    fs::remove_dir_all(&dir).ok();
}

fn wide_pool(params: usize, scope: Option<(f64, f64)>) -> String {
    let mut root = serde_json::json!({
        "params": (0..params)
            .map(|j| serde_json::json!({
                "name": format!("p{j}"),
                "min": 0,
                "max": 10,
                "distribution": "uniform",
            }))
            .collect::<Vec<_>>(),
        "designs": [],
    });
    if let Some((min, max)) = scope {
        root["scope"] = serde_json::json!(
            (0..params)
                .map(|j| serde_json::json!({
                    "name": format!("p{j}"),
                    "min": min,
                    "max": max,
                }))
                .collect::<Vec<_>>()
        );
    }
    serde_json::to_string(&root).unwrap()
}

#[test]
fn explore_on_50_parameters_is_batched() {
    // regression for issue #22: a fresh 50-parameter pool used to fail on
    // the greedy cross-product guard (6^50 combinations); batching splits
    // the run into one-design chunks and the run succeeds
    let dir = temp_dir("explore-50");
    let file = dir.join("designs.json");
    fs::write(&file, wide_pool(50, None)).unwrap();

    // human report works and keeps its wording
    let (out, stdout) = run(&dir, &["plhs", "--explore", "6", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("explored 6 designs (iteration 1), score "),
        "unexpected summary: {stdout}"
    );

    // a second fresh pool for the JSON report: one line, the new keys
    let file = dir.join("fresh.json");
    fs::write(&file, wide_pool(50, None)).unwrap();
    let (out, stdout) = run(&dir, &["plhs", "--explore", "6", "--json", "fresh.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["m"], 6);
    assert_eq!(v["chunks"], 6, "50 parameters must batch to m=1 chunks");
    assert_eq!(v["strategy"], "exact");
    assert_eq!(v["designs_after"], 6);
    assert_eq!(v["n_params"], 50);

    // the pool grew by the 6 new designs
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let designs = root["designs"].as_array().unwrap();
    assert_eq!(designs.len(), 6);
    assert_eq!(designs[0]["name"], "1.1");
    // a design carries its name plus one value per parameter
    assert_eq!(designs[0].as_object().unwrap().len(), 51);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn add_on_50_parameters_with_scope_is_batched() {
    // the scoped refinement path takes the same batching; the values
    // stay inside the scope
    let dir = temp_dir("add-50-scope");
    let file = dir.join("designs.json");
    fs::write(&file, wide_pool(50, Some((2.0, 8.0)))).unwrap();

    let (out, stdout) = run(&dir, &["plhs", "--add", "4", "--json", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["m"], 4);
    assert_eq!(v["chunks"], 4);
    assert_eq!(v["strategy"], "exact");
    assert_eq!(v["designs_after"], 4);

    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    for design in root["designs"].as_array().unwrap() {
        for j in 0..50 {
            let value = design[format!("p{j}")].as_f64().unwrap();
            assert!(
                (2.0..=8.0).contains(&value),
                "value {value} outside the scope"
            );
        }
    }
    fs::remove_dir_all(&dir).ok();
}
