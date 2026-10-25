//! Equal-probability-mass binning of the scope-transformed `[0, 1]` range.
//!
//! A parameter with `n` in-scope samples and `m` new designs to add is binned
//! into `n + m` bins. Because each in-scope sample occupies at most one bin,
//! at least `m` bins are always empty (pigeonhole) — those empty bins are the
//! candidate slots for the new designs.

use rand::Rng;

/// Number of bins for one parameter: in-scope samples plus new designs.
pub fn bin_count(n_in_scope: usize, m: usize) -> usize {
    n_in_scope + m
}

/// Bin index of a scope-transformed value `t` in `[0, 1]`.
///
/// Bins are left-closed; the last bin is right-closed, so `t = 1.0` belongs
/// to the last bin.
pub fn bin_index(t: f64, bins: usize) -> usize {
    debug_assert!((0.0..=1.0).contains(&t), "t={t} out of [0, 1]");
    debug_assert!(bins > 0);
    let idx = (t * bins as f64) as usize;
    idx.min(bins - 1)
}

/// Sorted unique indices of the bins occupied by the given transformed
/// values.
pub fn occupied_bins(ts: &[f64], bins: usize) -> Vec<usize> {
    let mut occupied: Vec<usize> = ts.iter().map(|&t| bin_index(t, bins)).collect();
    occupied.sort_unstable();
    occupied.dedup();
    occupied
}

/// Sorted indices of the empty bins, given the occupied ones.
pub fn empty_bins(bins: usize, occupied: &[usize]) -> Vec<usize> {
    let mut occupied = occupied.to_vec();
    occupied.sort_unstable();
    occupied.dedup();
    let mut empty = Vec::with_capacity(bins.saturating_sub(occupied.len()));
    let mut next = occupied.iter().copied();
    let mut current = next.next();
    for b in 0..bins {
        if Some(b) == current {
            current = next.next();
        } else {
            empty.push(b);
        }
    }
    empty
}

/// Draws the scope-transformed value of a random sample inside bin `bin`: a
/// uniform draw over the bin's probability mass (stratified sampling; the
/// inverse scope transform turns it into a distribution-shaped value).
pub fn candidate_t<R: Rng>(rng: &mut R, bin: usize, bins: usize) -> f64 {
    debug_assert!(bin < bins);
    let lo = bin as f64 / bins as f64;
    let hi = (bin + 1) as f64 / bins as f64;
    rng.random_range(lo..hi)
}

/// Uniformly samples `k` of the given bin indices without replacement,
/// returned in ascending order. Restricts a chunk's candidate set to a
/// searchable cross product when the empty bins of all parameters
/// together exceed the greedy budget.
pub fn sample_bins<R: Rng>(rng: &mut R, bins: &[usize], k: usize) -> Vec<usize> {
    debug_assert!(k <= bins.len());
    let mut pool: Vec<usize> = bins.to_vec();
    // partial Fisher-Yates: the first k entries are the draw
    for i in 0..k {
        let j = rng.random_range(i..pool.len());
        pool.swap(i, j);
    }
    pool.truncate(k);
    pool.sort_unstable();
    pool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_index_edges() {
        assert_eq!(bin_index(0.0, 5), 0);
        // edges are left-closed for the bin they open
        assert_eq!(bin_index(0.2, 5), 1);
        assert_eq!(bin_index(0.199_999, 5), 0);
        // the last bin is right-closed
        assert_eq!(bin_index(1.0, 5), 4);
        assert_eq!(bin_index(0.999_999, 5), 4);
    }

    #[test]
    fn empty_bins_from_occupied() {
        assert_eq!(empty_bins(5, &[0, 2, 4]), vec![1, 3]);
        assert_eq!(empty_bins(3, &[]), vec![0, 1, 2]);
        assert!(empty_bins(3, &[0, 1, 2]).is_empty());
        // duplicates in the input are tolerated
        assert_eq!(empty_bins(4, &[0, 0, 3, 3]), vec![1, 2]);
    }

    #[test]
    fn occupied_bins_dedup() {
        let ts = [0.05, 0.07, 0.5, 0.95];
        assert_eq!(occupied_bins(&ts, 10), vec![0, 5, 9]);
        assert_eq!(occupied_bins(&ts, 5), vec![0, 2, 4]);
    }

    #[test]
    fn candidate_t_lies_inside_bin() {
        let mut rng = rand::rng();
        for _ in 0..1000 {
            let bins = 17;
            let bin = 3;
            let t = candidate_t(&mut rng, bin, bins);
            let lo = bin as f64 / bins as f64;
            let hi = (bin + 1) as f64 / bins as f64;
            assert!(t >= lo && t < hi, "t={t} outside [{lo}, {hi})");
            assert_eq!(bin_index(t, bins), bin);
        }
    }

    #[test]
    fn bin_count_is_in_scope_plus_new() {
        assert_eq!(bin_count(15, 2), 17);
    }

    #[test]
    fn sample_bins_draws_distinct_bins_in_ascending_order() {
        let mut rng = rand::rng();
        let bins: Vec<usize> = (0..20).collect();
        let drawn = sample_bins(&mut rng, &bins, 7);
        assert_eq!(drawn.len(), 7);
        // ascending and strict (no duplicates)
        assert!(drawn.windows(2).all(|w| w[0] < w[1]));
        // a subset of the input bins
        for &b in &drawn {
            assert!(bins.contains(&b));
        }
        // k = len returns the full set, ascending like the input
        assert_eq!(sample_bins(&mut rng, &bins, 20), bins);
    }
}
