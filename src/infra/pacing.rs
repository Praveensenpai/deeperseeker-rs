use rand::Rng;
use std::time::Duration;

/// Compute a target inter-request gap that does not look like a metronome.
///
/// `jitter` is a fraction of the base gap: `0.75` yields a multiplier drawn
/// uniformly from `[0.25, 1.75]`. With probability `human_chance` an extra
/// pause of up to `human_max` seconds is appended, mimicking a person who
/// pauses to read before sending the next message.
pub fn effective_gap(base: f64, jitter: f64, human_chance: f64, human_max: f64) -> f64 {
    if base <= 0.0 {
        return 0.0;
    }

    let mut rng = rand::thread_rng();
    let j = jitter.clamp(0.0, 0.95);
    let multiplier = rng.gen_range((1.0 - j)..=(1.0 + j));
    let mut gap = base * multiplier;

    if human_chance > 0.0 && rng.gen_bool(human_chance.clamp(0.0, 1.0)) {
        let ceiling = human_max.max(1.0);
        gap += rng.gen_range(1.0..=ceiling);
    }

    gap.max(0.0)
}

/// Sleep for the remainder of `target` given that `elapsed` seconds have
/// already passed. Negative elapsed (clock skew) is treated as "no wait".
pub async fn sleep_remainder(elapsed: f64, target: f64) {
    if elapsed >= 0.0 && elapsed < target {
        let ms = ((target - elapsed) * 1000.0) as u64;
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_base_means_no_wait() {
        assert_eq!(effective_gap(0.0, 0.75, 0.5, 6.0), 0.0);
    }

    #[test]
    fn jitter_stays_within_bounds() {
        for _ in 0..500 {
            let gap = effective_gap(5.0, 0.75, 0.0, 6.0);
            assert!(gap >= 1.25 - f64::EPSILON, "gap {gap} below lower bound");
            assert!(gap <= 8.75 + f64::EPSILON, "gap {gap} above upper bound");
        }
    }

    #[test]
    fn human_pause_only_extends() {
        let mut extended = false;
        for _ in 0..2000 {
            let gap = effective_gap(5.0, 0.0, 1.0, 6.0);
            assert!(gap >= 5.0 - f64::EPSILON);
            assert!(gap <= 12.0 + f64::EPSILON);
            if gap > 5.0 {
                extended = true;
            }
        }
        assert!(extended, "human pause never triggered at chance 1.0");
    }
}
