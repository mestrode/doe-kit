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
    assert!(v.get("polish_ae_moves").is_none());
    assert!(v["score"].as_f64().unwrap() >= 0.0);
    // the space-filling energy is reported whether or not the pass ran
    assert!(v["ae_energy"].as_f64().is_some(), "ae_energy in {stdout}");
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
    // a greedy run is polished by default and reports what the pass did:
    // phase 1 (r²) and phase 2 (space-filling) each report their own
    // move counts and before/after values
    assert_eq!(v["polished"], true);
    assert!(
        v["polish_moves"].as_u64().is_some(),
        "polish_moves in {stdout}"
    );
    assert!(v["polish_score_before"].as_f64().is_some());
    assert!(v["polish_score_after"].as_f64().is_some());
    assert!(
        v["polish_ae_moves"].as_u64().is_some(),
        "polish_ae_moves in {stdout}"
    );
    assert!(v["polish_ae_before"].as_f64().is_some());
    assert!(v["polish_ae_after"].as_f64().is_some());
    assert!(v["ae_energy"].as_f64().is_some(), "ae_energy in {stdout}");
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
    let ae_moves = v["polish_ae_moves"].as_u64().expect("polish_ae_moves");
    assert!(
        moves + ae_moves > 0,
        "the drifted assignment must have improving swaps: {stdout}"
    );
    let before = v["polish_score_before"].as_f64().unwrap();
    let after = v["polish_score_after"].as_f64().unwrap();
    // phase 2 may spend r² headroom for space-filling — the invariant
    // is the per-pair τ guard of the combined objective, not a
    // monotone pooled score (3 parameters: 3 pairs of headroom)
    let tau_headroom = 3.0 * plhs::space::SELECTION_TAU;
    assert!(
        after <= before + tau_headroom + 1e-9,
        "polish broke the τ guard: {before} -> {after}"
    );
    assert_eq!(v["score"].as_f64().unwrap(), after);
    // the space-filling phase is monotone in the AE energy
    let ae_before = v["polish_ae_before"].as_f64().unwrap();
    let ae_after = v["polish_ae_after"].as_f64().unwrap();
    assert!(
        ae_after <= ae_before,
        "phase 2 must not raise AE: {ae_before} -> {ae_after}"
    );

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
        stdout.contains(&format!(
            ", polished with {} swap(s) (r2 was ",
            moves + ae_moves
        )),
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
    // the full report: the input definitions first, then the sampling
    // points, then the outputs
    assert!(stdout.contains("input parameters (2):"), "stdout: {stdout}");
    assert!(
        stdout.contains("x: uniform, range [0, 10], scope [2, 8] (60.0% of range)"),
        "stdout: {stdout}"
    );
    assert!(
        stdout
            .contains("y: gaussian, range [0, 10], mean 5, sigma 2, scope [3, 7] (40.0% of range)"),
        "stdout: {stdout}"
    );
    // the narrowed scope [3, 7] cuts 31.7% of the gaussian's mass:
    // the compact tier warns
    assert!(
        stdout.contains("    warning: 31.7% of the probability mass lies outside the scope"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("sampling points:"), "stdout: {stdout}");
    assert!(stdout.contains("outputs:"), "stdout: {stdout}");
    assert!(stdout.contains("energy: n=3,"), "stdout: {stdout}");
    // the fixture has a single result variable: no output pairs
    assert!(!stdout.contains("output pairs:"), "stdout: {stdout}");
    assert!(
        stdout.contains("min pairwise distance: "),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("mean nearest neighbor: "),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("discrepancy (CL2): "), "stdout: {stdout}");
    assert!(stdout.contains("pooled pair r2: "), "stdout: {stdout}");
    assert!(stdout.contains("empty decile slots: "), "stdout: {stdout}");
    assert!(
        stdout.contains("pairs: 1, max |pearson| "),
        "stdout: {stdout}"
    );
    // the detail blocks are behind --verbose
    assert!(!stdout.contains("x vs y: pearson "), "stdout: {stdout}");
    assert!(!stdout.contains("decile counts:"), "stdout: {stdout}");
    assert!(!stdout.contains("outer deciles: "), "stdout: {stdout}");
    assert!(!stdout.contains("in scope:"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), fixture());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_verbose_reports_the_full_details() {
    let dir = temp_dir("stats-verbose");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    // --verbose switches to the full tier: per-parameter descriptive
    // statistics and decile counts, the pair list, and the
    // per-parameter blocks of the result variables
    let (out, stdout) = run(&dir, &["stats", "--verbose", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("pool: 30 designs, 3 evaluated, 2 parameters (15 designs per parameter)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("x: n=30, min "), "stdout: {stdout}");
    assert!(stdout.contains("y: n=30, min "), "stdout: {stdout}");
    assert!(stdout.contains("decile counts: ["), "stdout: {stdout}");
    assert!(stdout.contains("outer deciles: "), "stdout: {stdout}");
    assert!(stdout.contains("x vs y: pearson "), "stdout: {stdout}");
    assert!(stdout.contains("empty decile slots: "), "stdout: {stdout}");
    assert!(stdout.contains("in scope: "), "stdout: {stdout}");
    assert!(stdout.contains("outside scope: "), "stdout: {stdout}");
    assert!(stdout.contains("energy: n=3,"), "stdout: {stdout}");
    assert!(stdout.contains("input parameters (2):"), "stdout: {stdout}");
    // the input section in the verbose tier shows the gaussian's lost
    // probability against the range and against the narrowed scope
    assert!(
        stdout.contains("    lost probability: 1.2% outside the range (0.6% below, 0.6% above)"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("    lost probability: 31.7% outside the scope (15.9% below, 15.9% above)"),
        "stdout: {stdout}"
    );
    // values instead of the warning
    assert!(!stdout.contains("warning:"), "stdout: {stdout}");
    assert!(stdout.contains("outputs:"), "stdout: {stdout}");

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), fixture());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_without_results_exits_1() {
    let dir = temp_dir("stats-empty");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"] = Value::Array(vec![]);
    let content = serde_json::to_string(&root).unwrap();
    fs::write(&file, &content).unwrap();

    // the full report runs the sections in order and the output
    // section needs results: nothing evaluated ends the run (the
    // `doe-stats sampling` view reports the sampling points of such
    // a pool instead)
    let (out, _) = run(&dir, &["stats", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no results to analyze"), "stderr: {stderr}");
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
            "  \"designs\": [{ \"name\": \"0.1\", \"x\": 5 }],\n",
            "  \"results\": [{ \"name\": \"0.1\", \"energy\": 7 }]\n",
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
        stdout.contains("pool: 1 designs, 1 evaluated, 1 parameters (1 designs per parameter)"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("min pairwise distance: n/a"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("mean pairwise distance: n/a"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("median pairwise distance: n/a"),
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
    // a single design cannot correlate with itself: no pair, no
    // maximum; 9 of the 10 decile slots of the single parameter stay
    // empty
    assert!(
        stdout.contains("empty decile slots: 9 of 10"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("pairs: 0, max |pearson| n/a, max |spearman| n/a"),
        "stdout: {stdout}"
    );
    // a single design cannot correlate with itself
    assert!(stdout.contains("pooled pair r2: n/a"), "stdout: {stdout}");
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
            "  ],\n",
            "  \"results\": [\n",
            "    { \"name\": \"0.1\", \"energy\": 1 },\n",
            "    { \"name\": \"0.2\", \"energy\": 2 },\n",
            "    { \"name\": \"0.3\", \"energy\": 3 }\n",
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
        stdout.contains("pool: 3 designs, 3 evaluated, 2 parameters (1.5 designs per parameter)"),
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
fn init_writes_the_abc_example_parameters() {
    let dir = temp_dir("init-abc");

    let (out, stdout) = run(&dir, &["init", "pool.json"]);
    assert!(out.status.success());
    assert!(
        stdout.contains("with 3 example parameters"),
        "unexpected summary: {stdout}"
    );

    // the created pool declares a uniform over [0, 10], b uniform over
    // [-1, 1], c gaussian over [0, 5] — the inputs of the analytic
    // example simulators
    let root: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pool.json")).unwrap()).unwrap();
    let params = root["params"].as_array().unwrap();
    assert_eq!(params.len(), 3);
    assert_eq!(params[0]["name"], "a");
    assert_eq!(params[0]["min"], 0);
    assert_eq!(params[0]["max"], 10);
    assert_eq!(params[0]["distribution"], "uniform");
    assert_eq!(params[1]["name"], "b");
    assert_eq!(params[1]["min"], -1);
    assert_eq!(params[1]["max"], 1);
    assert_eq!(params[1]["distribution"], "uniform");
    assert_eq!(params[2]["name"], "c");
    assert_eq!(params[2]["min"], 0);
    assert_eq!(params[2]["max"], 5);
    assert_eq!(params[2]["distribution"], "gaussian");
    assert_eq!(params[2]["mean"], 2.5);
    assert_eq!(params[2]["sigma"], 1);
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
fn stats_json_reports_the_compact_pool_analysis() {
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
    // the input section is single tier: always the full definitions
    let input = &v["input"];
    let input_params = input["parameters"].as_array().unwrap();
    assert_eq!(input_params.len(), 2);
    assert_eq!(input_params[0]["name"], "x");
    assert_eq!(input_params[0]["distribution"], "uniform");
    assert_eq!(input_params[0]["scope"], serde_json::json!([2.0, 8.0]));
    assert_eq!(input_params[0]["scope_fraction"], 0.6);
    assert_eq!(input_params[1]["mean"], 5.0);
    assert_eq!(input_params[1]["sigma"], 2.0);
    // the lost-probability facts ride along in both JSON tiers: no
    // threshold in JSON, and the narrowed scope carries the scope keys
    assert!(input_params[0].get("lost_probability").is_none());
    let lp = &input_params[1]["lost_probability"];
    assert!((lp["below_scope"].as_f64().unwrap() - 0.158655).abs() < 1e-4);
    assert!((lp["above_scope"].as_f64().unwrap() - 0.158655).abs() < 1e-4);
    // the default JSON is the compact tier: global metrics and the
    // summary fields, no per-parameter or pair detail
    let pool = &v["pool"];
    assert_eq!(pool["total"], 30);
    assert!(pool["min_pairwise_distance"].is_number());
    assert!(pool["mean_pairwise_distance"].is_number());
    assert!(pool["median_pairwise_distance"].is_number());
    assert!(pool["mean_nearest_neighbor"].is_number());
    assert!(pool["discrepancy_cl2"].is_number());
    assert!(pool["pooled_pair_r2"].is_number());
    assert!(pool["empty_decile_slots"].is_u64());
    assert!(pool["max_abs_pearson"].is_number());
    assert!(pool["max_abs_spearman"].is_number());
    assert_eq!(pool["n_pairs"], 1);
    assert!(
        pool.get("parameters").is_none(),
        "compact pool must not carry the parameters array"
    );
    assert!(
        pool.get("pairs").is_none(),
        "compact pool must not carry the pairs array"
    );
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "energy");
    assert_eq!(results[0]["n"], 3);
    assert_eq!(results[0]["total"], 30);
    // the wrapper inherits the per-result facts: the extreme designs
    // and the strongest input→output relations
    assert_eq!(results[0]["extremes"]["min_design"], "0.2");
    assert_eq!(results[0]["extremes"]["max_design"], "0.3");
    assert!(results[0]["relations"]["max_abs_pearson"]["parameter"].is_string());
    assert!(results[0]["relations"]["max_eta2"]["parameter"].is_string());
    assert!(
        results[0].get("parameters").is_none(),
        "compact results must not carry per-parameter blocks"
    );
    // the fixture has a single result variable: no pair, the compact
    // output-pairs object carries the count only
    let output_pairs = &v["output_pairs"];
    assert_eq!(output_pairs["n_pairs"], 0);
    assert!(
        output_pairs.get("pairs").is_none(),
        "compact output pairs must not carry the pairs array"
    );
    assert!(output_pairs["max_abs_pearson"].is_null());

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), fixture());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_json_verbose_reports_the_full_analysis() {
    let dir = temp_dir("stats-json-verbose");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    // --json --verbose returns the full object: the parameters and
    // pairs arrays and the per-parameter result blocks are back
    let (out, stdout) = run(&dir, &["stats", "--json", "--verbose", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "stats");
    let pool = &v["pool"];
    assert_eq!(pool["total"], 30);
    let params = pool["parameters"].as_array().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["name"], "x");
    assert_eq!(params[0]["n"], 30);
    assert_eq!(params[0]["deciles"].as_array().unwrap().len(), 10);
    assert!(params[0]["outer_deciles"].as_u64().is_some());
    assert!(pool["min_pairwise_distance"].is_number());
    assert!(pool["empty_decile_slots"].is_u64());
    assert!(pool["max_abs_pearson"].is_number());
    let pairs = pool["pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0]["a"], "x");
    assert_eq!(pairs[0]["b"], "y");
    assert!(pairs[0]["pearson"].is_number());
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["name"], "energy");
    assert_eq!(results[0]["n"], 3);
    assert_eq!(results[0]["parameters"].as_array().unwrap().len(), 2);
    // the fixture has a single result variable: the full pairs array
    // is present and empty
    assert!(v["output_pairs"]["pairs"].as_array().unwrap().is_empty());
    assert!(v["output_pairs"]["max_abs_spearman"].is_null());
    assert!(v["input"]["parameters"].as_array().unwrap().len() == 2);

    // read-only: the input file is untouched
    assert_eq!(fs::read_to_string(&file).unwrap(), fixture());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn stats_json_without_results_exits_1() {
    let dir = temp_dir("stats-json-empty");
    let file = dir.join("designs.json");
    let mut root: Value = serde_json::from_str(&fixture()).unwrap();
    root["results"] = Value::Array(vec![]);
    fs::write(&file, serde_json::to_string(&root).unwrap()).unwrap();

    // the full report needs results: without them the run fails before
    // any output (one line or none — here none)
    let (out, stdout) = run(&dir, &["stats", "--json", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout, "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no results to analyze"), "stderr: {stderr}");
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

// ---------------------------------------------------------------- reset

#[test]
fn reset_removes_unevaluated_designs_and_keeps_results() {
    let dir = temp_dir("reset");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();
    let original = fs::read(&file).unwrap();

    let (out, stdout) = run(&dir, &["reset", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout.trim_end(),
        "removed 27 designs (3 kept, 3 results)",
        "unexpected summary: {stdout}"
    );

    // backup equals the original input
    assert_eq!(fs::read(dir.join("designs.json.bak")).unwrap(), original);

    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let names: Vec<&str> = root["designs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["0.1", "0.2", "0.3"],
        "only the evaluated designs survive"
    );
    assert_eq!(root["results"].as_array().unwrap().len(), 3);
    assert!(root.get("params").is_some(), "params survive");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reset_all_empties_the_pool_entirely() {
    let dir = temp_dir("reset-all");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["reset", "--all", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout.trim_end(),
        "removed all 30 designs and 3 results",
        "unexpected summary: {stdout}"
    );

    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert!(root["designs"].as_array().unwrap().is_empty());
    assert!(root.get("results").is_none(), "the results array is gone");
    assert!(
        root.get("new_designs").is_none(),
        "the stale batch record is gone"
    );
    assert!(root.get("params").is_some(), "params survive");
    assert!(dir.join("designs.json.bak").exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reset_on_a_fresh_init_pool_is_a_no_op() {
    let dir = temp_dir("reset-init");
    let (out, _) = run(&dir, &["init", "pool.json"]);
    assert!(out.status.success());

    let (out, stdout) = run(&dir, &["reset", "pool.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.trim_end(), "removed 0 designs (0 kept, 0 results)");

    // and the second time too: the command is idempotent
    let (out, stdout) = run(&dir, &["reset", "pool.json"]);
    assert!(out.status.success());
    assert_eq!(stdout.trim_end(), "removed 0 designs (0 kept, 0 results)");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reset_supports_no_backup_and_reports_missing_file() {
    let dir = temp_dir("reset-flags");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, _) = run(&dir, &["reset", "--no-backup", "designs.json"]);
    assert!(out.status.success());
    assert!(!dir.join("designs.json.bak").exists());

    let (out, _) = run(&dir, &["reset", "does-not-exist.json"]);
    assert_eq!(out.status.code(), Some(2));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn reset_json_emits_one_machine_readable_line() {
    let dir = temp_dir("reset-json");
    let file = dir.join("designs.json");
    fs::write(&file, fixture()).unwrap();

    let (out, stdout) = run(&dir, &["reset", "--json", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["subcommand"], "reset");
    assert_eq!(v["mode"], "unevaluated");
    assert_eq!(v["file"], "designs.json");
    assert_eq!(v["designs_before"], 30);
    assert_eq!(v["designs_after"], 3);
    assert_eq!(v["results_before"], 3);
    assert_eq!(v["results_after"], 3);
    assert!(
        dir.join("designs.json.bak").exists(),
        "the pool is written and backed up as without --json"
    );

    let (out, stdout) = run(&dir, &["reset", "--all", "--json", "designs.json"]);
    assert!(out.status.success());
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["mode"], "all");
    assert_eq!(v["designs_before"], 3);
    assert_eq!(v["designs_after"], 0);
    assert_eq!(v["results_before"], 3);
    assert_eq!(v["results_after"], 0);
    fs::remove_dir_all(&dir).ok();
}

// -- polish (issue #48) ---------------------------------------------------

/// A small anti-correlated pool: two uniform parameters, `m` designs on
/// a perfect anti-diagonal (pooled pair r2 = 1.0), optionally with a
/// results entry marking the first design as evaluated.
fn polish_pool_json(m: usize, results: bool) -> String {
    let mut designs = String::new();
    for i in 0..m {
        let (x, y) = (
            (i as f64 + 0.5) * (10.0 / m as f64),
            (m as f64 - 0.5 - i as f64) * (10.0 / m as f64),
        );
        designs.push_str(&format!(
            r#"    {{ "name": "1.{}", "x": {x}, "y": {y} }}{sep}"#,
            i + 1,
            sep = if i + 1 < m { "," } else { "" }
        ));
    }
    let results = if results {
        r#", "results": [{ "name": "1.1", "linear": 0.5 }]"#
    } else {
        ""
    };
    format!(
        r#"{{
    "params": [
        {{ "name": "x", "min": 0, "max": 10 }},
        {{ "name": "y", "min": 0, "max": 10 }}
    ],
    "designs": [
{designs}
    ],
    "note": "unknown fields must survive"{results}
}}"#
    )
}

/// Sorted per-parameter values of the pool: the multiset every swap
/// preserves structurally.
fn sorted_columns(file: &std::path::Path, params: &[&str]) -> Vec<Vec<f64>> {
    let root: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    params
        .iter()
        .map(|p| {
            let mut v: Vec<f64> = root["designs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d[p].as_f64().unwrap())
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v
        })
        .collect()
}

#[test]
fn polish_refines_a_pool_writes_and_backs_up() {
    let dir = temp_dir("polish-pool");
    let file = dir.join("designs.json");
    fs::write(&file, polish_pool_json(10, false)).unwrap();
    let original = fs::read(&file).unwrap();
    let multisets = sorted_columns(&file, &["x", "y"]);

    let (out, stdout) = run(&dir, &["polish", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("polished 10 designs, r2 1.000000 -> "),
        "unexpected summary: {stdout}"
    );
    assert!(stdout.contains("moves"), "summary: {stdout}");

    // backup equals the pre-polish input
    assert_eq!(fs::read(dir.join("designs.json.bak")).unwrap(), original);
    // every swap preserves the per-parameter value multisets
    assert_eq!(sorted_columns(&file, &["x", "y"]), multisets);
    // unknown fields survive
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(root["note"], "unknown fields must survive");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn polish_json_reports_the_keys_and_the_stats_objective() {
    let dir = temp_dir("polish-json");
    let file = dir.join("designs.json");
    fs::write(&file, polish_pool_json(10, false)).unwrap();

    let (out, stdout) = run(&dir, &["polish", "--json", "designs.json"]);
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    // the exact key set of the issue contract
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "ae_after",
            "ae_before",
            "designs",
            "elapsed_ms",
            "evaluated_skipped",
            "file",
            "movable",
            "moves",
            "r2_after",
            "r2_before",
            "subcommand",
        ]
    );
    assert_eq!(v["subcommand"], "polish");
    assert_eq!(v["file"], "designs.json");
    assert_eq!(v["designs"], 10);
    assert_eq!(v["movable"], 10);
    assert_eq!(v["evaluated_skipped"], 0);
    assert!(v["moves"].as_u64().unwrap() > 0);
    let before = v["r2_before"].as_f64().unwrap();
    let after = v["r2_after"].as_f64().unwrap();
    assert!(
        (before - 1.0).abs() < 1e-9,
        "anti-diagonal starts at r2 1: {before}"
    );
    assert!(after < 0.1, "the descent must scatter the pairs: {after}");

    // the full-surface contract: r2_after is the pooled pair r2 the
    // doe-stats sampling report computes on the polished pool (Pearson
    // is invariant under the per-parameter affine normalization)
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let designs = root["designs"].as_array().unwrap();
    let column = |p: &str| -> Vec<f64> { designs.iter().map(|d| d[p].as_f64().unwrap()).collect() };
    let (xs, ys) = (column("x"), column("y"));
    let pooled = plhs::scoring::pearson(&xs, &ys).powi(2);
    assert!(
        (pooled - after).abs() < 1e-9,
        "stats pooled r2 {pooled} != reported r2_after {after}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn polish_keeps_evaluated_designs_fixed() {
    let dir = temp_dir("polish-fixed");
    let file = dir.join("designs.json");
    fs::write(&file, polish_pool_json(8, true)).unwrap();

    let (out, stdout) = run(&dir, &["polish", "--json", "designs.json"]);
    assert!(out.status.success(), "stderr: {stdout}");
    let v: Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(v["designs"], 8);
    assert_eq!(v["movable"], 7);
    assert_eq!(v["evaluated_skipped"], 1);

    // the evaluated design is fixed context: its values never move
    let root: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let evaluated = &root["designs"][0];
    assert_eq!(evaluated["name"], "1.1");
    assert_eq!(evaluated["x"], 0.625);
    assert_eq!(evaluated["y"], 9.375);
    // the results entry passes through untouched
    assert_eq!(root["results"][0]["linear"], 0.5);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn polish_noop_exits_zero_without_touching_the_file() {
    let dir = temp_dir("polish-noop");
    let file = dir.join("designs.json");
    fs::write(&file, polish_pool_json(1, false)).unwrap();
    let original = fs::read(&file).unwrap();

    let (out, stdout) = run(&dir, &["polish", "designs.json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("(0 moves)"), "summary: {stdout}");
    // nothing changed: no rewrite, no backup
    assert_eq!(fs::read(&file).unwrap(), original);
    assert!(!dir.join("designs.json.bak").exists());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn polish_flags_and_exit_codes() {
    let dir = temp_dir("polish-flags");
    let file = dir.join("designs.json");
    fs::write(&file, polish_pool_json(10, false)).unwrap();

    // --no-backup refines without a backup
    let (out, _) = run(&dir, &["polish", "--no-backup", "designs.json"]);
    assert!(out.status.success());
    assert!(!dir.join("designs.json.bak").exists());

    // missing file: I/O error
    let (out, _) = run(&dir, &["polish", "does-not-exist.json"]);
    assert_eq!(out.status.code(), Some(2));

    // structurally invalid pool: validation error
    fs::write(
        &file,
        r#"{"params": [{"name": "x", "min": 0, "max": 10}], "designs": [{"name": "1.1"}]}"#,
    )
    .unwrap();
    let (out, stderr) = run(&dir, &["polish", "designs.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).starts_with("error:"),
        "stderr: {stderr}"
    );
    fs::remove_dir_all(&dir).ok();
}
