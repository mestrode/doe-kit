//! Golden test: the `plhs` binary on the committed fixture with the pinned
//! default seed must reproduce the committed expected output byte-for-byte.

use std::fs;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_plhs");

#[test]
fn golden_output_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!("plhs-golden-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("designs.json");
    fs::write(&input, fs::read("tests/fixtures/input.json").unwrap()).unwrap();

    let out = Command::new(BIN)
        .args(["--add", "2", "designs.json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // byte-for-byte: same binary, same input, same seed => same output
    let produced = fs::read_to_string(dir.join("designs.json")).unwrap();
    let expected = fs::read_to_string("tests/fixtures/expected.json").unwrap();
    assert_eq!(
        produced, expected,
        "golden output differs; if the algorithm changed intentionally, \
         regenerate tests/fixtures/expected.json with the plhs binary"
    );
    fs::remove_dir_all(&dir).ok();
}
