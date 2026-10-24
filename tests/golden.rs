//! Golden test: the full pipeline on the committed fixture with a pinned seed
//! must reproduce the committed expected output byte-for-byte.

use std::fs;
use std::path::Path;

use serde_json::Value;

use plhs::json;

const M: usize = 2;
const SEED: u64 = 42;
const CAP: u64 = 1_000_000;

#[test]
fn golden_output_is_reproduced() {
    let input = fs::read_to_string(Path::new("tests/fixtures/input.json")).unwrap();
    let expected = fs::read_to_string(Path::new("tests/fixtures/expected.json")).unwrap();

    let mut root: Value = serde_json::from_str(&input).unwrap();
    let params = json::parse_params(&root).unwrap();
    let designs = json::parse_designs(&root, &params).unwrap();
    let aug = plhs::augment(&params, &designs, M, SEED, CAP).unwrap();
    json::append_new_designs(&mut root, &params, &aug.new_designs).unwrap();
    let output = serde_json::to_vec_pretty(&root).unwrap();

    // byte-for-byte: same code, same input, same seed => same output
    assert_eq!(
        String::from_utf8(output).unwrap(),
        expected,
        "golden output differs; if the algorithm changed intentionally, \
         regenerate tests/fixtures/expected.json with the plhs binary"
    );
}

#[test]
fn golden_explore_output_is_reproduced() {
    let input = fs::read_to_string(Path::new("tests/fixtures/input.json")).unwrap();
    let expected = fs::read_to_string(Path::new("tests/fixtures/expected_explore.json")).unwrap();

    let mut root: Value = serde_json::from_str(&input).unwrap();
    let params = json::parse_params(&root).unwrap();
    let designs = json::parse_designs(&root, &params).unwrap();
    let aug = plhs::explore(&params, &designs, M, SEED, CAP).unwrap();
    json::append_new_designs(&mut root, &params, &aug.new_designs).unwrap();
    let output = serde_json::to_vec_pretty(&root).unwrap();

    assert_eq!(
        String::from_utf8(output).unwrap(),
        expected,
        "golden explore output differs; if the algorithm changed intentionally, \
         regenerate tests/fixtures/expected_explore.json with the plhs binary \
         run as: plhs --explore 2 tests/fixtures/input.json"
    );
}
