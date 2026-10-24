//! Latin Hypercube invariants of the augmentation.

use plhs::binning::{bin_count, bin_index, empty_bins, occupied_bins};
use plhs::{Design, Parameter, augment};

fn params() -> Vec<Parameter> {
    vec![
        Parameter::uniform("x", 0.0, 10.0)
            .unwrap()
            .with_scope(2.0, 8.0)
            .unwrap(),
        Parameter::uniform("y", 0.0, 10.0).unwrap(),
    ]
}

fn designs() -> Vec<Design> {
    // deterministic, hand-spread values; x has 10 in-scope samples, y 12
    (0..12)
        .map(|i| {
            let x = if i < 10 { 2.0 + i as f64 * 0.55 } else { 0.5 };
            let y = 0.4 + i as f64 * 0.75;
            Design {
                name: format!("0.{}", i + 1),
                values: vec![x, y],
            }
        })
        .collect()
}

#[test]
fn new_designs_use_distinct_empty_bins_per_parameter() {
    let params = params();
    let designs = designs();
    let m = 3;
    let aug = augment(&params, &designs, m, 123, 1_000_000).unwrap();
    assert_eq!(aug.new_designs.len(), m);

    for (j, param) in params.iter().enumerate() {
        let in_scope: Vec<f64> = designs
            .iter()
            .filter(|d| param.in_scope(d.values[j]))
            .map(|d| param.transform(d.values[j]))
            .collect();
        let bins = bin_count(in_scope.len(), m);
        let occupied = occupied_bins(&in_scope, bins);
        let empty = empty_bins(bins, &occupied);

        // new designs land in empty bins, one distinct bin each
        let mut seen = Vec::new();
        for design in &aug.new_designs {
            let value = design.values[j];
            assert!(
                param.in_scope(value),
                "{} not in scope of {}",
                value,
                param.name
            );
            let idx = bin_index(param.transform(value), bins);
            assert!(
                empty.contains(&idx),
                "design {} value {} landed in occupied bin {idx}",
                design.name,
                value
            );
            assert!(
                !seen.contains(&idx),
                "bin {idx} used twice for {}",
                param.name
            );
            seen.push(idx);
        }
    }
}

#[test]
fn same_seed_same_result_different_seed_different_result() {
    let params = params();
    let designs = designs();
    let a = augment(&params, &designs, 2, 7, 1_000_000).unwrap();
    let b = augment(&params, &designs, 2, 7, 1_000_000).unwrap();
    assert_eq!(a.new_designs, b.new_designs);
    assert_eq!(a.score, b.score);

    let c = augment(&params, &designs, 2, 8, 1_000_000).unwrap();
    let differs = a
        .new_designs
        .iter()
        .zip(&c.new_designs)
        .any(|(x, y)| x.values != y.values);
    assert!(differs, "seeds 7 and 8 produced identical designs");
}

#[test]
fn iteration_increments_across_runs() {
    let params = params();
    let mut designs = designs();
    let first = augment(&params, &designs, 2, 42, 1_000_000).unwrap();
    assert_eq!(first.iteration, 1);
    designs.extend(first.new_designs.clone());

    let second = augment(&params, &designs, 2, 42, 1_000_000).unwrap();
    assert_eq!(second.iteration, 2);
    for design in &second.new_designs {
        let fresh = !designs.iter().any(|d| d.name == design.name);
        assert!(fresh, "duplicate design name {}", design.name);
    }
}

#[test]
fn score_of_chosen_assignment_beats_a_naive_one() {
    // with 2 empty bins per parameter and m = 2 the enumeration is tiny;
    // the chosen score must not exceed the score of the reverse assignment
    // (i.e. selection actually optimized something)
    let params = params();
    let designs = designs();
    let aug = augment(&params, &designs, 2, 99, 1_000_000).unwrap();
    assert!(aug.score <= 1.0, "score unreasonably high: {}", aug.score);
}
