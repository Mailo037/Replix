//! How the recorder recovers when a capture fails: it waits longer after each failure in
//! a row, and stops using an encoder that keeps failing right after it starts.

use std::collections::HashMap;
use std::time::Duration;

/// A capture that ran this long counts as having worked; the next failure starts afresh.
pub(super) const HEALTHY: Duration = Duration::from_secs(60);
/// A capture that failed sooner than this, with its target still there, failed to start.
const EARLY: Duration = Duration::from_secs(20);
/// Early failures after which an encoder is passed over until Replix restarts.
const STRIKES: u32 = 2;
/// The waits before trying again, failure after failure.
const WAITS: [u64; 5] = [5, 10, 20, 40, 60];

#[derive(Default)]
pub(super) struct Health {
    /// Failures in a row.
    failures: u32,
    /// Early failures per encoder id.
    strikes: HashMap<&'static str, u32>,
}

impl Health {
    /// Notes a capture that stopped on its own. Says whether the encoder is now passed over.
    pub fn failed(&mut self, encoder: &'static str, ran: Duration, target_still_there: bool) -> bool {
        self.failures += 1;
        if ran >= EARLY || !target_still_there {
            return false;
        }
        let strikes = self.strikes.entry(encoder).or_default();
        *strikes += 1;
        *strikes == STRIKES
    }

    /// The capture has run long enough to count as working.
    pub fn ran_well(&mut self, encoder: &'static str) {
        self.failures = 0;
        self.strikes.remove(encoder);
    }

    pub fn wait(&self) -> Duration {
        let step = (self.failures.max(1) as usize - 1).min(WAITS.len() - 1);
        Duration::from_secs(WAITS[step])
    }

    pub fn usable(&self, encoder: &str) -> bool {
        self.strikes.get(encoder).is_none_or(|s| *s < STRIKES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_grow_with_each_failure_and_reset_after_a_good_run() {
        let mut h = Health::default();
        assert_eq!(h.wait(), Duration::from_secs(5));
        for _ in 0..3 {
            h.failed("nvenc", Duration::from_secs(300), true);
        }
        assert_eq!(h.wait(), Duration::from_secs(20));
        for _ in 0..10 {
            h.failed("nvenc", Duration::from_secs(300), true);
        }
        assert_eq!(h.wait(), Duration::from_secs(60), "capped");
        h.ran_well("nvenc");
        assert_eq!(h.wait(), Duration::from_secs(5));
    }

    #[test]
    fn an_encoder_that_keeps_failing_at_start_is_passed_over() {
        let mut h = Health::default();
        assert!(!h.failed("amf", Duration::from_secs(2), true));
        assert!(h.usable("amf"));
        assert!(h.failed("amf", Duration::from_secs(3), true), "second strike");
        assert!(!h.usable("amf") && h.usable("x264"));
        // A game that closed, or a capture that ran a while, is not the encoder's fault.
        let mut h = Health::default();
        h.failed("amf", Duration::from_secs(2), false);
        h.failed("amf", Duration::from_secs(120), true);
        h.failed("amf", Duration::from_secs(2), false);
        assert!(h.usable("amf"));
    }
}
