//! Golden test: the `simio` binary on the committed fixture must
//! reproduce the committed expected output byte-for-byte.

use std::fs;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_simio");

#[test]
fn golden_test_output_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!("simio-golden-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("pool.json");

    // the input fixture is the pinned init+explore output of the
    // sampling crate (its own golden test keeps it stable); if that
    // fixture changes intentionally, this input must be updated too
    fs::write(
        &input,
        fs::read("tests/fixtures/input_explored.json").unwrap(),
    )
    .unwrap();

    let out = Command::new(BIN)
        .args(["test", "pool.json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // byte-for-byte: same binary, same input => same output
    let produced = fs::read_to_string(dir.join("pool.json")).unwrap();
    let expected = fs::read_to_string("tests/fixtures/expected_test_results.json").unwrap();
    assert_eq!(
        produced, expected,
        "golden output differs; if the analytic functions changed intentionally, \
         regenerate tests/fixtures/expected_test_results.json with the simio binary"
    );
    fs::remove_dir_all(&dir).ok();
}
