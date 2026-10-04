//! Where a new message identity comes from.
//!
//! Replay needs a **new** `message_id`: `messages.message_id` is a primary key and the original row still holds
//! the terminal state that justified the replay, so reusing the identity would mean rewriting history rather than
//! adding to it. The declared machine has no `EXPIRED -> *` or `DEAD_LETTER -> *` edge, so a replay is a new
//! message that begins at `CREATED` like any other - which is why it needs a new identity rather than a reset.
//!
//! The source is injected for the same reason the clock is: the bus must not reach for ambient state, and a
//! production source would have to read a clock or a random device. `crates/bus` performs no I/O, so the
//! production source belongs to whoever wires the bus into the shell.

/// Mints message identities for replay.
pub trait IdSource {
    /// The identity for the next replayed message. Must not return a value it has returned before.
    fn next_message_id(&self) -> String;
}

/// An identity source that counts, so a test can name the identity it expects.
///
/// Deterministic and free of I/O, and the only implementation this crate ships. It is deliberately **not** a
/// production source: two runs of a real system must not mint the same identity, and this one would.
pub struct FixedIdSource {
    prefix: String,
    next: std::cell::Cell<u64>,
}

impl FixedIdSource {
    /// A source whose identities are `{prefix}_1`, `{prefix}_2`, and so on.
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            next: std::cell::Cell::new(1),
        }
    }
}

impl IdSource for FixedIdSource {
    fn next_message_id(&self) -> String {
        let n = self.next.get();
        self.next.set(n + 1);
        format!("{}_{n}", self.prefix)
    }
}
