//! A tiny value-delta gate: fire an emit only when a monotonically-growing
//! value has advanced by at least `min_delta` since the last emit, or when a
//! terminal tick (100% / final byte) forces one. Pulled out of the shell so
//! the throttling decision is unit-tested instead of inlined per call site.

pub struct EmitThrottle {
    min_delta: u64,
    last: Option<u64>,
}

impl EmitThrottle {
    pub fn new(min_delta: u64) -> Self {
        Self {
            min_delta,
            last: None,
        }
    }

    /// True when `value` should be emitted: the first call, any call whose
    /// value advanced by >= `min_delta` since the last emit, or any `terminal`
    /// call. Records the approved value so the next delta measures from it.
    pub fn should_emit(&mut self, value: u64, terminal: bool) -> bool {
        let fire = terminal
            || match self.last {
                None => true,
                Some(prev) => value.saturating_sub(prev) >= self.min_delta,
            };
        if fire {
            self.last = Some(value);
        }
        fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_first_then_every_min_delta_and_on_terminal() {
        let mut t = EmitThrottle::new(5);
        assert!(t.should_emit(0, false), "first call always emits");
        assert!(!t.should_emit(3, false), "below delta: suppressed");
        assert!(t.should_emit(5, false), "reached delta from last emit (0)");
        assert!(!t.should_emit(9, false), "9-5=4 < 5: suppressed");
        assert!(
            t.should_emit(9, true),
            "terminal forces an emit even below delta"
        );
        // terminal recorded 9 as last, so next delta measures from 9
        assert!(!t.should_emit(13, false), "13-9=4 < 5");
        assert!(t.should_emit(14, false), "14-9=5 >= 5");
    }

    #[test]
    fn large_deltas_for_byte_counts() {
        let mut t = EmitThrottle::new(4_000_000);
        assert!(t.should_emit(0, false));
        assert!(!t.should_emit(3_999_999, false));
        assert!(t.should_emit(4_000_000, false));
    }

    #[test]
    fn a_value_below_last_is_suppressed_without_underflow() {
        // Regression: saturating_sub must never underflow (which would yield a
        // spuriously large delta and fire) when a caller passes a value BELOW
        // the last emitted value (e.g. a progress counter that resets or goes
        // non-monotonic). The guard is the saturating_sub already in place.
        let mut t = EmitThrottle::new(5);
        assert!(t.should_emit(10, false)); // first call always emits; last = 10
        assert!(
            !t.should_emit(3, false),
            "a value below last is suppressed, never underflows"
        );
        assert!(t.should_emit(3, true), "terminal always fires");
    }
}
