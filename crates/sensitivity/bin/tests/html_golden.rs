//! Golden test: the HTML report of the committed pool fixture must
//! reproduce the committed expected page byte-for-byte. The page is
//! fully deterministic (no timestamps), so the same input always
//! yields the same bytes.

use std::fs;

const FIXTURE: &str = "tests/fixtures/pool.json";
const EXPECTED: &str = "tests/fixtures/expected_report.html";

/// The full analysis of the fixture pool — the same pipeline the
/// binary's bare report runs — formatted by the library.
#[test]
fn golden_html_report_is_reproduced_by_the_library() {
    let root = doe_pool::read(std::path::Path::new(FIXTURE)).unwrap().1;
    let params = doe_pool::format::parse_params(&root).unwrap();
    let designs = doe_pool::format::parse_designs(&root, &params).unwrap();
    let results = doe_pool::format::parse_results(&root).unwrap();
    let input = doe_stats::analyze_inputs(&params);
    let pool = doe_stats::analyze_pool(&params, &designs).unwrap();
    let stats = doe_stats::analyze(&params, &designs, &results).unwrap();
    let pairs = doe_stats::analyze_result_pairs(&designs, &results).unwrap();
    let data = doe_stats_bin::ReportData {
        input: &input,
        pool: &pool,
        results: &stats,
        pairs: &pairs,
        evaluated: results.len(),
    };

    let produced =
        doe_stats_bin::html_report("pool.json", &data, &params, &designs, &results).unwrap();
    let expected = fs::read_to_string(EXPECTED)
        .unwrap_or_else(|_| panic!("missing {EXPECTED}; regenerate it with the doe-stats binary"));
    assert_eq!(
        produced, expected,
        "golden HTML differs; if the report format changed intentionally, \
         regenerate {EXPECTED} with the doe-stats binary (--html)"
    );
}
