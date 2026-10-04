//! The bus's retry and dispatch policy, as configuration rather than as locked constants.
//!
//! The authoritative values are in `schemas/mcf-v2/bus-policies.json`. This module ships the same values as a
//! `Default` implementation so a caller that configures nothing still gets them, and the contract gate reads
//! both and requires them to agree - the same arrangement `crates/council` uses for
//! `schemas/council-v1/council-policies.json`, and for the same reason: a policy that lives only in code is not
//! configuration, and a policy that lives only in JSON is a document nothing enforces.
//!
//! The numbers are `pub const` in a shape the gate can read back, so the comparison is mechanical rather than
//! a comment that says "matching".

/// The shipped `bus-policies.json:dispatch.max_attempts`.
pub const MAX_ATTEMPTS: i64 = 5;
/// The shipped `bus-policies.json:dispatch.batch_size`.
pub const BATCH_SIZE: i64 = 32;
/// The shipped `bus-policies.json:backoff.base_seconds`.
pub const BASE_SECONDS: i64 = 2;
/// The shipped `bus-policies.json:backoff.multiplier`.
pub const MULTIPLIER: i64 = 2;
/// The shipped `bus-policies.json:backoff.cap_seconds`.
pub const CAP_SECONDS: i64 = 300;

/// How many delivery attempts a message gets before its budget is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchPolicy {
    /// Send while the recorded attempt count is below this.
    pub max_attempts: i64,
    /// The most queue entries one dispatch pass claims.
    pub batch_size: i64,
}

impl Default for DispatchPolicy {
    fn default() -> Self {
        DispatchPolicy {
            max_attempts: MAX_ATTEMPTS,
            batch_size: BATCH_SIZE,
        }
    }
}

/// How long to wait before the next delivery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    /// The delay before the first attempt, in seconds.
    pub base_seconds: i64,
    /// The factor applied per further attempt.
    pub multiplier: i64,
    /// The ceiling on any one delay, in seconds.
    pub cap_seconds: i64,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        BackoffPolicy {
            base_seconds: BASE_SECONDS,
            multiplier: MULTIPLIER,
            cap_seconds: CAP_SECONDS,
        }
    }
}

impl BackoffPolicy {
    /// The delay before attempt `attempt_no`, counting from 1.
    ///
    /// Bounded and deterministic from the stored attempt count and these values alone: no jitter and no
    /// sampling of the wall clock, so two runs over the same stored state compute the same delay. The
    /// multiplication saturates at the cap rather than wrapping, because a corrupted attempt count must not be
    /// able to produce a delay that is negative or absurd.
    ///
    /// An `attempt_no` below 1 is treated as the first attempt. That is the same reading `DispatchPolicy`
    /// applies: the count is a record of what has happened, and a record that says "none yet" describes a first
    /// attempt whichever way it is spelled.
    pub fn delay_seconds(&self, attempt_no: i64) -> i64 {
        let base = self.base_seconds.max(0);
        let cap = self.cap_seconds.max(0);
        let steps = attempt_no.saturating_sub(1).max(0);
        let mut delay = base.min(cap);
        let multiplier = self.multiplier.max(1);
        for _ in 0..steps {
            // Saturating rather than checked-and-unwrapped: reaching the cap is the normal end of this loop, so
            // the overflow branch is not an error to report, it is the bound doing its job.
            match delay.checked_mul(multiplier) {
                Some(next) if next < cap => delay = next,
                _ => return cap,
            }
        }
        delay
    }
}
