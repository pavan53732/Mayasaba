//! The dispatch decision, as a pure function.
//!
//! The whole of the bus's judgement about a queue entry is here, and it is a function of the recorded attempt
//! count and the policy - nothing else. Not the clock, not the database, not the transport's mood. That is what
//! makes the retry behaviour reviewable: the question "how many times will this be tried, and what happens
//! then?" is answered by reading one match arm rather than by tracing a loop.
//!
//! The decision is taken from **stored** state, which is the design's requirement: "Backoff is bounded and
//! deterministic from stored retry policy/attempt state." A dispatcher that counted attempts in memory would
//! reset its budget on every restart, so a message that crashed the process on every attempt would be retried
//! forever.

use crate::policy::DispatchPolicy;

/// What to do with one due queue entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchDecision {
    /// Hand it to the transport, as attempt `attempt_no` (counting from 1).
    Send {
        /// Which attempt this will be, for the attempt row and the backoff that follows it.
        attempt_no: i64,
    },
    /// Stop trying. The attempt budget is exhausted, so the message terminates through the declared
    /// `QUEUED -> EXPIRED` transition and a dead letter records the final error.
    Expire {
        /// How many attempts were made, for the dead letter's `attempts` column.
        attempts: i64,
    },
}

/// Decide what to do with a queue entry that has recorded `attempts` delivery attempts.
///
/// `attempts` is read from `outbox.attempts`, so it survives a restart. A negative count - which the column's
/// type admits even though nothing writes one - is read as "none yet", because the alternative is a message
/// that can never be sent.
pub fn decide(attempts: i64, policy: &DispatchPolicy) -> DispatchDecision {
    let made = attempts.max(0);
    if made >= policy.max_attempts.max(0) {
        DispatchDecision::Expire { attempts: made }
    } else {
        DispatchDecision::Send {
            attempt_no: made.saturating_add(1),
        }
    }
}
