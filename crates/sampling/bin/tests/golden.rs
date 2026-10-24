//! Golden tests: the `sampling` binary on the committed fixtures must
//! reproduce the committed expected outputs byte-for-byte.

use std::fs;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_sampling");

fn assert_golden(dir: &Path, args: &[&str], expected_fixture: &str) {
    let input = dir.join("designs.json");
    fs::write(&input, fs::read("tests/fixtures/input.json").unwrap()).unwrap();

    let out = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // byte-for-byte: same binary, same input, same seed => same output
    let produced = fs::read_to_string(dir.join("designs.json")).unwrap();
    let expected = fs::read_to_string(expected_fixture).unwrap();
    assert_eq!(
        produced, expected,
        "golden output differs; if the algorithm changed intentionally, \
         regenerate {expected_fixture} with the sampling binary"
    );
    fs::remove_dir_all(dir).ok();
}

#[test]
fn golden_add_output_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!("sampling-golden-add-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    assert_golden(
        &dir,
        &["plhs", "--add", "2", "designs.json"],
        "tests/fixtures/expected.json",
    );
}

#[test]
fn golden_explore_output_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!("sampling-golden-explore-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    assert_golden(
        &dir,
        &["plhs", "--explore", "2", "designs.json"],
        "tests/fixtures/expected_explore.json",
    );
}

#[test]
fn golden_explore_on_a_fresh_init_pool_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!(
        "sampling-golden-explore-init-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // create a fresh pool, then explore it; both steps go through the
    // binary so the committed fixture pins the whole flow byte-for-byte
    let steps: [&[&str]; 2] = [
        &["init", "designs.json"],
        &["plhs", "--explore", "10", "designs.json"],
    ];
    for args in steps {
        let out = Command::new(BIN)
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let produced = fs::read_to_string(dir.join("designs.json")).unwrap();
    let expected = fs::read_to_string("tests/fixtures/expected_explore_init.json").unwrap();
    assert_eq!(
        produced, expected,
        "explore-on-init output differs; if the algorithm changed intentionally, \
         regenerate tests/fixtures/expected_explore_init.json with the sampling binary"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn golden_init_output_is_reproduced_by_the_binary() {
    let dir = std::env::temp_dir().join(format!("sampling-golden-init-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // init refuses to overwrite, so the target file must not pre-exist;
    // the shared helper seeds the file first, hence a direct invocation
    let out = Command::new(BIN)
        .args(["init", "designs.json"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // byte-for-byte: the skeleton is static, so the created file must
    // match the committed fixture exactly
    let produced = fs::read_to_string(dir.join("designs.json")).unwrap();
    let expected = fs::read_to_string("tests/fixtures/expected_init.json").unwrap();
    assert_eq!(
        produced, expected,
        "init output differs; if the skeleton changed intentionally, \
         regenerate tests/fixtures/expected_init.json with the sampling binary"
    );
    fs::remove_dir_all(&dir).ok();
}
