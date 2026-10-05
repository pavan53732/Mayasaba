//! The shell's bus runtime: the managed state, the transport it holds, and the retry it owns.
//!
//! The shell is transport, not domain logic (AGENTS.md section 11). This module holds the bus and the
//! infrastructure the bus deliberately refuses to contain, because `crates/bus` performs no I/O and takes its
//! clock, identity source and transport as caller-supplied traits.
//!
//! # What this module deliberately does not do
//!
//! **Nothing here dispatches.** There is no transport until M3, and a dispatcher running against a transport
//! that cannot deliver would attempt every due message, exhaust its retry budget and dead-letter genuine work
//! to satisfy a scheduler with nothing to do (DEC-073). [`NotConnectedTransport`] exists so that a future
//! caller has something honest to hold, and `nothing_in_the_shell_dispatches_automatically` fails if a dispatch
//! call site is ever added to this file or to `main.rs`.
//!
//! **Nothing here writes a delivery state.** The four state columns carry no SQL `CHECK` constraint and cannot
//! gain one, because there is no migration runner and `CREATE TABLE IF NOT EXISTS` cannot alter an existing
//! table. The bus API is therefore the only permitted route to a state change, and this module never touches
//! those columns itself.

use std::path::Path;
use std::sync::{Arc, Mutex};

use mayasaba_bus::{BackoffPolicy, Bus, BusError, Clock, Transport, TransportError};

/// The transport the shell holds while no agent transport exists.
///
/// It reports that it is not connected rather than pretending to succeed. A transport that returned `Ok`
/// without delivering would let the bus mark a message `DISPATCHED` that no agent ever received, which is
/// precisely the fabrication this work exists to avoid: `DISPATCHED` means "handed to the transport", and
/// handing it to nothing is not delivery.
// The four items below are the shell's bus runtime surface, introduced in Tranche 1 ahead of their callers.
// They are exercised by this module's tests and are consumed by the diagnostic and replay handlers in Tranches
// 2 and 3. `dead_code` is allowed rather than the surface being left out or artificially called from `main`,
// because a call invented only to silence the lint would be a call that does nothing - and removing these
// allows is part of landing those handlers.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct NotConnectedTransport;

impl Transport for NotConnectedTransport {
    fn send(&mut self, _envelope_json: &str) -> Result<(), TransportError> {
        // The bus's existing transport-failure semantics, unchanged: no new error code is invented for a
        // condition the vocabulary already describes (DEC-055).
        Err(TransportError::Unavailable(
            "no agent transport is connected; agent adapters arrive in M3".to_string(),
        ))
    }
}

/// One recorded capacity retry, so a caller can log or surface what it waited for.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryAttempt {
    /// Which retry this was, counting from 1.
    pub attempt: i64,
    /// How long the caller was asked to wait before it, in seconds.
    pub waited_seconds: i64,
    /// When the retry was scheduled, from the injected clock.
    pub scheduled_at: String,
}

/// Retry an operation that was refused for capacity, and only for capacity.
///
/// `CAPACITY_EXCEEDED` means "not yet", never "failed" (DEC-068). The bus refuses rather than retrying, because
/// it performs no I/O and holds no opinion about when a caller should give up; the shell owns that decision.
///
/// Three properties are load-bearing and each is tested:
///
/// - **Bounded.** At most `max_attempts` calls are made, and the last capacity refusal is returned rather than
///   looping. An unbounded retry would be a hang with no failure report.
/// - **Capacity-only.** Any other error is returned immediately and never retried. Retrying a malformed
///   envelope or a storage failure would turn a permanent fault into a slow one.
/// - **Deterministic delay.** The wait comes from the bus's own `BackoffPolicy`, computed from the attempt
///   number alone, so two runs over the same refusals wait the same amounts. The wait itself is injected, so
///   the tests observe the schedule without sleeping.
#[allow(dead_code)]
pub fn retry_on_capacity<T>(
    backoff: &BackoffPolicy,
    clock: &dyn Clock,
    max_attempts: i64,
    attempts: &mut Vec<RetryAttempt>,
    wait: &mut dyn FnMut(i64),
    op: &mut dyn FnMut() -> Result<T, BusError>,
) -> Result<T, BusError> {
    let mut attempt: i64 = 1;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(refused @ BusError::CapacityExceeded { .. }) => {
                if attempt >= max_attempts {
                    return Err(refused);
                }
                let waited_seconds = backoff.delay_seconds(attempt);
                attempts.push(RetryAttempt {
                    attempt,
                    waited_seconds,
                    scheduled_at: clock.now_rfc3339(),
                });
                wait(waited_seconds);
                attempt += 1;
            }
            // Returned unchanged, not retried and not reworded: the caller must see the error the bus
            // reported rather than a substitute.
            Err(other) => return Err(other),
        }
    }
}

/// The shell's bus, holding its own `Storage` connection.
///
/// Its own connection, never shared with `ProjectService`'s (DEC-072). Sharing one would serialise all project
/// and bus work behind a single mutex and would couple the bus's transaction boundaries to the project
/// service's; separate connections let SQLite's own locking arbitrate, which is what the 5000 ms busy timeout
/// set by `Storage::open` exists for (DEC-067).
#[allow(dead_code)]
pub struct BusShell {
    bus: Bus,
}

#[allow(dead_code)]
impl BusShell {
    /// Open the bus over its own connection to `path`.
    pub fn open(path: &Path) -> Result<Self, mayasaba_storage::StorageError> {
        let storage = mayasaba_storage::Storage::open(path)?;
        Ok(BusShell {
            bus: Bus::new(storage),
        })
    }

    /// The bus, for a caller that owns the lock.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// The bus, mutably, for a caller that owns the lock.
    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }
}

/// Shared shell state, in the shape a `spawn_blocking` closure needs.
///
/// `tauri::State` borrows for the duration of a command and is not `'static`, so it cannot be moved into a
/// blocking task. An `Arc<Mutex<..>>` can, which is why the bus is managed in this shape rather than directly
/// (DEC-072).
pub type SharedBus = Arc<Mutex<BusShell>>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_db(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "mayasaba_shell_{tag}_{}_{n}.sqlite3",
            std::process::id()
        ))
    }

    fn capacity_refusal() -> BusError {
        BusError::CapacityExceeded {
            pending: 10,
            limit: 10,
        }
    }

    #[test]
    fn the_transport_reports_that_it_is_not_connected() {
        // Not `Ok`: a transport that claimed success without delivering would let the bus mark a message
        // DISPATCHED that no agent received.
        let mut transport = NotConnectedTransport;
        let error = transport
            .send("{}")
            .expect_err("a transport with no connection must not report success");
        assert_eq!(error.code(), "ADAPTER_UNAVAILABLE");
        assert!(!error.detail().is_empty());
    }

    #[test]
    fn retry_stops_at_the_bound_and_returns_the_refusal() {
        let mut attempts = Vec::new();
        let mut waits = Vec::new();
        let mut calls = 0;

        let outcome: Result<(), BusError> = retry_on_capacity(
            &BackoffPolicy::default(),
            &mayasaba_core::bus_runtime::CoreClock,
            4,
            &mut attempts,
            &mut |seconds| waits.push(seconds),
            &mut || {
                calls += 1;
                Err(capacity_refusal())
            },
        );

        assert!(
            matches!(outcome, Err(BusError::CapacityExceeded { .. })),
            "the bound must return the capacity refusal, not a substitute error"
        );
        assert_eq!(
            calls, 4,
            "exactly max_attempts calls, never an unbounded loop"
        );
        assert_eq!(attempts.len(), 3, "one fewer retry than calls");
        assert_eq!(waits.len(), 3);
    }

    #[test]
    fn retry_waits_are_monotonic_under_the_backoff_policy() {
        let mut attempts = Vec::new();
        let mut waits = Vec::new();

        let _: Result<(), BusError> = retry_on_capacity(
            &BackoffPolicy::default(),
            &mayasaba_core::bus_runtime::CoreClock,
            6,
            &mut attempts,
            &mut |seconds| waits.push(seconds),
            &mut || Err(capacity_refusal()),
        );

        assert_eq!(waits.len(), 5);
        for pair in waits.windows(2) {
            assert!(pair[1] >= pair[0], "backoff must not decrease: {waits:?}");
        }
        // The schedule is recorded with a stamp from the injected clock, so a caller can log it.
        assert!(attempts.iter().all(|a| !a.scheduled_at.is_empty()));
        assert_eq!(attempts[0].attempt, 1);
    }

    #[test]
    fn a_non_capacity_error_is_returned_unchanged_and_not_retried() {
        let mut attempts = Vec::new();
        let mut calls = 0;

        let outcome: Result<(), BusError> = retry_on_capacity(
            &BackoffPolicy::default(),
            &mayasaba_core::bus_runtime::CoreClock,
            5,
            &mut attempts,
            &mut |_| panic!("a non-capacity error must not be waited on"),
            &mut || {
                calls += 1;
                Err(BusError::Malformed {
                    column: "messages.delivery_state".to_string(),
                    detail: "not a declared state".to_string(),
                })
            },
        );

        assert_eq!(calls, 1, "a permanent fault must not be retried");
        assert!(attempts.is_empty());
        match outcome {
            Err(BusError::Malformed { column, .. }) => {
                assert_eq!(column, "messages.delivery_state")
            }
            other => panic!("the original error must be returned unchanged, got {other:?}"),
        }
    }

    #[test]
    fn a_success_after_refusals_returns_the_value() {
        let mut attempts = Vec::new();
        let mut calls = 0;

        let outcome: Result<i32, BusError> = retry_on_capacity(
            &BackoffPolicy::default(),
            &mayasaba_core::bus_runtime::CoreClock,
            5,
            &mut attempts,
            &mut |_| {},
            &mut || {
                calls += 1;
                if calls < 3 {
                    Err(capacity_refusal())
                } else {
                    Ok(7)
                }
            },
        );

        assert_eq!(outcome.ok(), Some(7));
        assert_eq!(calls, 3);
        assert_eq!(attempts.len(), 2);
    }

    #[test]
    fn constructing_the_shell_state_dead_letters_nothing() {
        let path = temp_db("rest");
        let shell = BusShell::open(&path).expect("the bus store opens");

        let dead: i64 = shell
            .bus()
            .storage()
            .conn()
            .query_row("SELECT COUNT(*) FROM dead_letters", [], |row| row.get(0))
            .expect("dead letter count");
        let queued: i64 = shell
            .bus()
            .storage()
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("message count");

        assert_eq!(
            dead, 0,
            "constructing the state must not dead-letter anything"
        );
        assert_eq!(
            queued, 0,
            "constructing the state must not enqueue anything"
        );
        assert_eq!(shell.bus().backlog().expect("backlog"), 0);

        let _ = std::fs::remove_file(&path);
    }

    /// A structural guard, not a behavioural one: it fails if a dispatch call site is ever added.
    ///
    /// The needle is assembled with `concat!` so that this assertion's own source text does not contain the
    /// literal it searches for. Without that, `include_str!` would match the test itself and the guard would
    /// pass for the wrong reason - or, worse, always fail.
    #[test]
    fn nothing_in_the_shell_dispatches_automatically() {
        let needle = concat!("dispatch", "_due");
        for (name, source) in [
            ("bus_shell.rs", include_str!("bus_shell.rs")),
            ("main.rs", include_str!("main.rs")),
        ] {
            assert!(
                !source.contains(needle),
                "{name} calls {needle}. No transport exists until M3, and a dispatcher against a transport that \
                 cannot deliver would exhaust the retry budget and dead-letter genuine messages (DEC-073)."
            );
        }
    }
}
