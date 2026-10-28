//! Golden tests: the `doe-stats` binary on the committed pool fixture
//! must reproduce the committed expected report byte-for-byte.

use std::fs;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_doe-stats");

#[test]
fn golden_full_report_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!(
        "doe-stats-golden-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("pool.json");
    fs::write(&input, fs::read("tests/fixtures/pool.json").unwrap()).unwrap();

    let out = Command::new(BIN)
        .arg("pool.json")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // byte-for-byte: same binary, same input => same compact report
    let produced = String::from_utf8_lossy(&out.stdout).to_string();
    let expected = fs::read_to_string("tests/fixtures/expected_full_report.txt").unwrap();
    assert_eq!(
        produced, expected,
        "golden output differs; if the report format changed intentionally, \
         regenerate tests/fixtures/expected_full_report.txt with the \
         doe-stats binary"
    );
    fs::remove_dir_all(&dir).ok();
}
