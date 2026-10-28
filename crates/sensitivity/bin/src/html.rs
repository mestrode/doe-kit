//! The HTML report: the full statistics JSON plus the raw design and
//! result values embedded into a single self-contained HTML page.
//!
//! Presentation only, mirroring the crate's stance: every displayed
//! statistic is computed by `doe-stats` in Rust; the embedded
//! JavaScript renders and never computes. The payload is the full
//! (`--verbose`) tier of the JSON report — the HTML report has no
//! compact tier — extended by a `points` object holding the raw
//! per-design parameter and result values the diagrams plot, and by
//! `critical_params`, the input-section lost-probability warnings
//! (precomputed in Rust so the threshold stays out of the browser).
//!
//! Charts use Plotly.js from a CDN: the page needs internet to render
//! diagrams, while the statistics and tables render fully offline.
//!
//! The template is [`TEMPLATE`]; the payload is injected at
//! [`DATA_PLACEHOLDER`] by splitting the template (not by string
//! replacement), so a payload that itself contains the placeholder or
//! a `</script>` sequence cannot corrupt the page.

use doe_stats::{InputStats, PoolStats, ResultPairStats, VariableStats};
use doe_types::{Design, Parameter, Results};
use serde_json::{Value, json};

use crate::ReportData;

/// The single-file HTML template. Contains [`DATA_PLACEHOLDER`]
/// exactly once.
const TEMPLATE: &str = include_str!("template.html");

/// The one placeholder the template carries: where the JSON payload
/// is injected.
const DATA_PLACEHOLDER: &str = "__DATA__";

// ---------------------------------------------------------------------
// payload
// ---------------------------------------------------------------------

/// The parameter names whose compact input section would warn: the
/// scope loss of a Gaussian parameter exceeds the critical fraction
/// (same rule as [`crate::human_input`]; the threshold stays in Rust,
/// the browser only renders the flag).
fn critical_params(input: &InputStats) -> Vec<String> {
    input
        .parameters
        .iter()
        .filter(|p| {
            p.lost_probability
                .as_ref()
                .is_some_and(|lp| crate::scope_loss(lp) > crate::LOST_PROBABILITY_CRITICAL)
        })
        .map(|p| p.name.clone())
        .collect()
}

/// The raw values the diagrams plot: one entry per design, aligned
/// arrays per parameter and per result variable (`null` where a design
/// has no entry or no value for a variable). Pure data — the browser
/// draws scatter points from it, it never computes statistics.
fn points_value(
    params: &[Parameter],
    designs: &[Design],
    raw_results: Option<(&[Results], &[String])>,
) -> Value {
    let mut parameters = serde_json::Map::new();
    for (i, p) in params.iter().enumerate() {
        parameters.insert(
            p.name.clone(),
            json!(designs.iter().map(|d| d.values[i]).collect::<Vec<_>>()),
        );
    }
    let mut points = json!({
        "designs": designs.iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
        "parameters": parameters,
    });
    if let Some((raw, variables)) = raw_results {
        let index: std::collections::HashMap<&str, &std::collections::BTreeMap<String, f64>> =
            raw.iter().map(|r| (r.name.as_str(), &r.values)).collect();
        let mut series = serde_json::Map::new();
        for v in variables {
            series.insert(
                v.clone(),
                json!(
                    designs
                        .iter()
                        .map(|d| {
                            index
                                .get(d.name.as_str())
                                .and_then(|values| values.get(v).copied())
                        })
                        .collect::<Vec<Option<f64>>>()
                ),
            );
        }
        points["results"] = Value::Object(series);
    }
    points
}

/// Renders the template with `payload` injected: the template is split
/// at the placeholder (once, by position) and the JSON is inserted
/// with every `</` escaped to `<\/`, so a payload can never close the
/// embedding script tag early.
fn render(payload: Value) -> Result<String, serde_json::Error> {
    debug_assert_eq!(
        TEMPLATE.matches(DATA_PLACEHOLDER).count(),
        1,
        "the template must carry the data placeholder exactly once"
    );
    let json = payload.to_string().replace("</", "<\\/");
    let (head, tail) = TEMPLATE
        .split_once(DATA_PLACEHOLDER)
        .expect("the template carries the data placeholder");
    Ok(format!("{head}{json}{tail}"))
}

// ---------------------------------------------------------------------
// reports
// ---------------------------------------------------------------------

/// Builds the single-section HTML report of the input view: the
/// parameter definitions and their lost-probability warnings. Works
/// without designs and without results.
pub fn html_input(file: &str, input: &InputStats) -> Result<String, serde_json::Error> {
    render(json!({
        "subcommand": "input",
        "file": file,
        "input": serde_json::to_value(input)?,
        "critical_params": critical_params(input),
    }))
}

/// Builds the single-section HTML report of the sampling view: the
/// full pool statistics plus the per-design parameter values the
/// scatter diagrams plot.
pub fn html_sampling(
    file: &str,
    evaluated: usize,
    pool: &PoolStats,
    params: &[Parameter],
    designs: &[Design],
) -> Result<String, serde_json::Error> {
    render(json!({
        "subcommand": "sampling",
        "file": file,
        "evaluated": evaluated,
        "pool": serde_json::to_value(pool)?,
        "points": points_value(params, designs, None),
    }))
}

/// Builds the single-section HTML report of the output view: the full
/// per-result statistics, the output pairs, and the per-design values
/// the parameter-vs-result diagrams plot.
pub fn html_output(
    file: &str,
    evaluated: usize,
    results: &[VariableStats],
    pairs: &ResultPairStats,
    params: &[Parameter],
    designs: &[Design],
    raw_results: &[Results],
) -> Result<String, serde_json::Error> {
    let variables: Vec<String> = results.iter().map(|v| v.name.clone()).collect();
    render(json!({
        "subcommand": "output",
        "file": file,
        "evaluated": evaluated,
        "results": serde_json::to_value(results)?,
        "output_pairs": serde_json::to_value(pairs)?,
        "points": points_value(params, designs, Some((raw_results, &variables))),
    }))
}

/// Builds the full HTML report: all three sections (input, sampling,
/// output) and the complete plot data.
pub fn html_report(
    file: &str,
    data: &ReportData,
    params: &[Parameter],
    designs: &[Design],
    raw_results: &[Results],
) -> Result<String, serde_json::Error> {
    let variables: Vec<String> = data.results.iter().map(|v| v.name.clone()).collect();
    render(json!({
        "subcommand": "all",
        "file": file,
        "evaluated": data.evaluated,
        "input": serde_json::to_value(data.input)?,
        "critical_params": critical_params(data.input),
        "pool": serde_json::to_value(data.pool)?,
        "results": serde_json::to_value(data.results)?,
        "output_pairs": serde_json::to_value(data.pairs)?,
        "points": points_value(params, designs, Some((raw_results, &variables))),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the committed fixture pool and runs the full analysis —
    /// the same pipeline the binary's bare report executes.
    fn fixture_data() -> (
        ReportData<'static>,
        Vec<Parameter>,
        Vec<Design>,
        Vec<Results>,
    ) {
        let text = std::fs::read_to_string("tests/fixtures/pool.json").unwrap();
        let root: Value = serde_json::from_str(&text).unwrap();
        let params = doe_pool::format::parse_params(&root).unwrap();
        let designs = doe_pool::format::parse_designs(&root, &params).unwrap();
        let results = doe_pool::format::parse_results(&root).unwrap();
        let input = doe_stats::analyze_inputs(&params);
        let pool = doe_stats::analyze_pool(&params, &designs).unwrap();
        let (stats, pairs) = (
            doe_stats::analyze(&params, &designs, &results).unwrap(),
            doe_stats::analyze_result_pairs(&designs, &results).unwrap(),
        );
        // leak the analysis so ReportData can borrow 'static in tests
        let input: &'static InputStats = Box::leak(Box::new(input));
        let pool: &'static PoolStats = Box::leak(Box::new(pool));
        let stats: &'static Vec<VariableStats> = Box::leak(Box::new(stats));
        let pairs: &'static ResultPairStats = Box::leak(Box::new(pairs));
        (
            ReportData {
                input,
                pool,
                results: stats,
                pairs,
                evaluated: results.len(),
            },
            params,
            designs,
            results,
        )
    }

    /// Extracts the embedded JSON payload from a rendered page.
    fn payload(html: &str) -> Value {
        let start = "<script type=\"application/json\" id=\"report-data\">".len()
            + html
                .find("<script type=\"application/json\" id=\"report-data\">")
                .expect("the page embeds the payload");
        let end = start
            + html[start..]
                .find("</script>")
                .expect("payload script closes");
        serde_json::from_str(&html[start..end]).expect("the embedded payload parses")
    }

    #[test]
    fn full_report_html_contains_the_three_tabs_and_the_payload() {
        let (data, params, designs, results) = fixture_data();
        let html = html_report("pool.json", &data, &params, &designs, &results).unwrap();

        assert!(html.starts_with("<!DOCTYPE html>"));
        for section in ["section-input", "section-sampling", "section-output"] {
            assert!(
                html.contains(&format!("id=\"{section}\"")),
                "missing {section}"
            );
        }
        let p = payload(&html);
        assert_eq!(p["subcommand"], "all");
        assert_eq!(p["file"], "pool.json");
        assert_eq!(p["evaluated"], 4);
        for key in ["input", "pool", "results", "output_pairs", "points"] {
            assert!(p[key].is_object() || p[key].is_array(), "missing {key}");
        }
        assert_eq!(p["points"]["designs"].as_array().unwrap().len(), 4);
        assert_eq!(p["points"]["parameters"]["x"].as_array().unwrap().len(), 4);
        assert_eq!(p["points"]["results"]["a"][0].as_f64(), Some(1.0));
    }

    #[test]
    fn input_report_carries_only_the_input_payload() {
        let (data, _, _, _) = fixture_data();
        let html = html_input("pool.json", data.input).unwrap();

        assert!(html.contains("id=\"section-input\""));
        let p = payload(&html);
        assert_eq!(p["subcommand"], "input");
        for key in ["pool", "results", "output_pairs", "points", "evaluated"] {
            assert!(p[key].is_null(), "input view must not carry {key}");
        }
        // the page boots with a single tab: absent sections are pruned
        // by the embedded script at runtime, so the static markup must
        // not pre-activate them
        assert!(!html.contains("class=\"active\""));
    }

    #[test]
    fn sampling_report_embeds_the_design_points() {
        let (data, params, designs, _) = fixture_data();
        let html = html_sampling("pool.json", 4, data.pool, &params, &designs).unwrap();

        let p = payload(&html);
        assert_eq!(p["subcommand"], "sampling");
        assert!(p["pool"].is_object());
        // the full tier: the pairs are not pruned to a count
        assert!(p["pool"]["pairs"].is_array());
        assert_eq!(p["points"]["designs"][1], "1.2");
        assert_eq!(p["points"]["parameters"]["y"][3], 7.0);
        // sampling never reads result values
        assert!(p["points"]["results"].is_null());
    }

    #[test]
    fn output_report_embeds_result_values_with_gaps() {
        let (data, params, designs, mut results) = fixture_data();
        // a design without an evaluated entry yields null in its series
        results.truncate(3);
        let html = html_output(
            "pool.json",
            3,
            data.results,
            data.pairs,
            &params,
            &designs,
            &results,
        )
        .unwrap();

        let p = payload(&html);
        assert_eq!(p["subcommand"], "output");
        let b: Vec<_> = p["points"]["results"]["b"].as_array().unwrap().clone();
        assert_eq!(b[2], 6.0);
        assert!(b[3].is_null());
    }

    #[test]
    fn critical_params_flags_heavy_scope_loss() {
        // centered gaussian: only 1.2% outside the range, no warning
        let (data, _, _, _) = fixture_data();
        let html = html_input("pool.json", data.input).unwrap();
        let p = payload(&html);
        assert_eq!(p["critical_params"].as_array().unwrap().len(), 0);

        // a heavily truncated gaussian must be flagged
        let text = r#"{"params":[{"name":"g","min":0,"max":10,"mean":8,"sigma":2,"distribution":"gaussian"}],
                      "designs":[{"name":"1.1","g":5}],"results":[]}"#;
        let root: Value = serde_json::from_str(text).unwrap();
        let params = doe_pool::format::parse_params(&root).unwrap();
        let input = doe_stats::analyze_inputs(&params);
        let html = html_input("pool.json", &input).unwrap();
        let p = payload(&html);
        assert_eq!(p["critical_params"], json!(["g"]));
    }

    #[test]
    fn a_script_breaking_payload_stays_embedded_intact() {
        let (data, params, mut designs, results) = fixture_data();
        // the </ sequence (e.g. in a design name) must not close the
        // embedding script tag, and the placeholder must not be
        // replaced inside the payload
        designs[0].name = "</script><b>__DATA__".to_owned();
        let html = html_report("pool.json", &data, &params, &designs, &results).unwrap();

        let p = payload(&html);
        assert_eq!(p["points"]["designs"][0], "</script><b>__DATA__");
        // the page carries exactly one script tag of the payload kind
        assert_eq!(
            html.matches("<script type=\"application/json\" id=\"report-data\">")
                .count(),
            1
        );
    }

    #[test]
    fn the_report_is_deterministic() {
        let (data, params, designs, results) = fixture_data();
        let a = html_report("pool.json", &data, &params, &designs, &results).unwrap();
        let b = html_report("pool.json", &data, &params, &designs, &results).unwrap();
        assert_eq!(a, b);
    }
}
