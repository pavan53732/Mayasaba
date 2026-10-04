//! The bus's injectable clock, following `crates/council/src/clock.rs`.
//!
//! The bus reads time to answer exactly one question - is a queue entry due? - and it must not read the wall
//! clock to answer it, because a test that cannot set the time cannot test a backoff.
//!
//! # Why this crate ships no formatter
//!
//! A timestamp is only compared here, never parsed and never constructed. `next_attempt_at` is computed by
//! SQLite, which is already in the path and already knows the calendar, and the comparison against `now` is a
//! string comparison of two fixed-width UTC stamps. That is why this module has no `format_rfc3339_utc`: the
//! repository's one RFC3339 formatter is `crates/council/src/clock.rs`, and the bus may not depend on
//! `crates/council` - so shipping a second copy of the civil-date algorithm would be a second source of truth
//! for time formatting in exchange for nothing this crate needs.
//!
//! The consequence is stated rather than hidden: **the bus ships no production clock.** `FixedClock` exists for
//! tests, and whoever wires the bus into the shell supplies the implementation that reads the system time. That
//! implementation must produce the same shape `FixedClock` does - `YYYY-MM-DDTHH:MM:SSZ`, UTC, fixed width -
//! because the due comparison is a string comparison and would silently misorder stamps that are spelled
//! differently. [`Clock::now_rfc3339`] states that contract, and the storage tests pin that SQLite produces the
//! same shape for `next_attempt_at`.

/// A source of the current time, so that "is this due?" is testable.
pub trait Clock {
    /// The current time as `YYYY-MM-DDTHH:MM:SSZ`: UTC, fixed width, no fractional seconds and no offset.
    ///
    /// The shape is part of the contract rather than a formatting preference. A due check compares this string
    /// with a stored `next_attempt_at` lexicographically, which orders instants correctly only while both sides
    /// are the same fixed-width UTC spelling.
    fn now_rfc3339(&self) -> String;
}

/// A clock that returns a fixed stamp, so a test can put the bus at any instant.
#[derive(Debug, Clone)]
pub struct FixedClock {
    stamp: String,
}

impl FixedClock {
    /// A clock stopped at `stamp`.
    pub fn new(stamp: impl Into<String>) -> Self {
        FixedClock {
            stamp: stamp.into(),
        }
    }
}

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        self.stamp.clone()
    }
}
