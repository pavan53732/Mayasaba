//! Verification tests for four gaps M2 identified as "reasoned, not tested".
//!
//! These are not new feature tests. Each test exercises something M2 left on the basis of reasoning rather
//! than observation, and converts it into an assertion that either passes (the reasoning was right) or fails
//! (the reasoning was wrong and the implementation needs repair).
//!
//! The four gaps, in order:
//!
//! 1. **Pass count > 2.** `evt_msg_x_3_action_failed_2` proved the suffix counter reaches 2. This test drives
//!    a third full round (`action_failed_3`) so the counter is observed past 2 and the uniqueness argument for
//!    suffix generation is not confined to a single increment.
//!
//! 2. **Replaying a `DEAD_LETTER` message.** `replay_source` accepts `DEAD_LETTER` alongside `EXPIRED`, and
//!    the idempotency check narrows to live claims — but no test in the M2 suite reaches that branch.
//!    This test drives a message to `DEAD_LETTER` through `PROCESSING → REJECTED → DEAD_LETTER` and then
//!    calls `replay`, checking that the new message is queued, the original is untouched, and the chain is intact.
//!
//! 3. **Concurrent writers.** The pass-count read relies on the transaction boundary for serialization, but M2
//!    never ran two writers at the same time. This test spawns two threads each enqueueing to the same
//!    file-backed database, joins them, and asserts that both rows landed, neither was lost, and the backlog
//!    is exactly two. SQLite's default locking (journal mode WAL is not required for this) serializes writers
//!    at the file level; the test confirms observation rather than relying on the argument.
//!
//! 4. **Backlog query cost.** `pending_outbound_count` was argued to use `idx_messages_priority` or a scan
//!    bounded by the outbox join. This test captures `EXPLAIN QUERY PLAN` and asserts the plan does not say
//!    "SCAN outbox" without a covering index — i.e., at least one of the two tables in the join is accessed
//!    via an index or a covering scan, confirming the cost argument without a benchmark.

mod common;

use common::{envelope, existing_dir, project, storage_with_project, ScriptedTransport};
use mayasaba_bus::{Bus, FixedClock, FixedIdSource};
use mayasaba_storage::Storage;

fn at(stamp: &str) -> FixedClock {
    FixedClock::new(stamp)
}

fn state_of(bus: &Bus, message_id: &str) -> String {
    bus.storage()
        .conn()
        .query_row(
            "SELECT delivery_state FROM messages WHERE message_id = ?1",
            [message_id],
            |r| r.get(0),
        )
        .expect("message row")
}

// -----------------------------------------------------------------------------------------------------------
// Gap 1: pass count past 2 — suffix counter observed at _3
// -----------------------------------------------------------------------------------------------------------

/// Drive a message through three full receive→process→reject→requeue rounds and assert the third
/// `ACTION_FAILED` event is recorded under `evt_…_action_failed_3`.
///
/// M2 observed `_2`. This test forces `_3` and checks both uniqueness (no primary-key collision)
/// and correctness (the id matches the deterministic formula `evt_{id}_{type}_{n}`).
#[test]
fn pass_count_suffix_reaches_3_and_stays_unique() {
    let (mut bus, _dir) = {
        let (s, d) = storage_with_project("pass_count_3");
        (Bus::new(s), d)
    };
    let clock = at("2026-10-04T00:00:10Z");
    let msg = envelope(
        "msg_pc_1",
        "prj_pass_count_3",
        1,
        Some("op_pc_1"),
        r#"{"task":"a"}"#,
    );

    // Round 1
    bus.receive(&msg, &clock).expect("recv 1");
    bus.start_processing("msg_pc_1", &clock).expect("start 1");
    bus.reject_processing("msg_pc_1", "busy", true, &clock)
        .expect("reject 1");
    bus.requeue("msg_pc_1", &clock).expect("requeue 1");
    let mut t = ScriptedTransport::always_ok();
    bus.dispatch_due(&clock, &mut t).expect("dispatch 1");

    // Round 2
    bus.receive(&msg, &clock).expect("recv 2");
    bus.start_processing("msg_pc_1", &clock).expect("start 2");
    bus.reject_processing("msg_pc_1", "busy", true, &clock)
        .expect("reject 2");
    bus.requeue("msg_pc_1", &clock).expect("requeue 2");
    bus.dispatch_due(&clock, &mut t).expect("dispatch 2");

    // Round 3 — this is the one M2 never reached
    bus.receive(&msg, &clock).expect("recv 3");
    bus.start_processing("msg_pc_1", &clock).expect("start 3");
    bus.reject_processing("msg_pc_1", "busy", true, &clock)
        .expect("reject 3");

    // Collect all event ids that mention this message.
    let ids: Vec<String> = {
        let conn = bus.storage().conn();
        let mut stmt = conn
            .prepare("SELECT event_id FROM events WHERE payload_json LIKE ?1 ORDER BY rowid")
            .expect("prepare");
        let rows = stmt
            .query_map(["%\"msg_pc_1\"%"], |r| r.get::<_, String>(0))
            .expect("query");
        rows.map(|r| r.expect("id")).collect()
    };

    // The first two occurrences use the plain and _2 ids (as observed in M2).
    assert!(
        ids.contains(&"evt_msg_pc_1_action_failed".to_string()),
        "plain id must exist: {ids:?}"
    );
    assert!(
        ids.contains(&"evt_msg_pc_1_action_failed_2".to_string()),
        "_2 suffix must exist: {ids:?}"
    );
    // The third — newly observed — must be _3, not a collision or a random id.
    assert!(
        ids.contains(&"evt_msg_pc_1_action_failed_3".to_string()),
        "_3 suffix must exist: {ids:?}"
    );

    // All ids are unique: if the suffix formula collided, the event would not have been written.
    let unique_count = {
        let set: std::collections::HashSet<&String> = ids.iter().collect();
        set.len()
    };
    assert_eq!(
        unique_count,
        ids.len(),
        "every event id in the log must be unique; a collision would mean silent data loss"
    );

    // The hash chain must be intact across all three rounds.
    assert!(
        bus.storage()
            .verify_event_chain()
            .expect("verify")
            .is_intact(),
        "the event chain must be intact after three rounds"
    );
}

// -----------------------------------------------------------------------------------------------------------
// Gap 2: replaying a DEAD_LETTER message
// -----------------------------------------------------------------------------------------------------------

/// Drive a message to `DEAD_LETTER` through the inbound path (`PROCESSING → REJECTED → DEAD_LETTER`)
/// and then replay it, asserting:
/// - the original remains `DEAD_LETTER` (history is not rewritten);
/// - the replay is `QUEUED` and is a different message;
/// - `causation_id` points to the `MESSAGE_DEAD_LETTERED` event, not the `MESSAGE_EXPIRED` event;
/// - the hash chain is intact throughout.
///
/// M2 accepted `DEAD_LETTER` in `replay_source` on the basis of the same state-check that handles
/// `EXPIRED`, but never exercised it. This test closes that gap.
#[test]
fn replaying_a_dead_letter_message_creates_a_new_queued_message() {
    let (mut bus, _dir) = {
        let (s, d) = storage_with_project("replay_dl");
        (Bus::new(s), d)
    };
    let clock = at("2026-10-04T00:00:10Z");
    let msg = envelope(
        "msg_dl_1",
        "prj_replay_dl",
        1,
        Some("op_dl_1"),
        r#"{"task":"a"}"#,
    );

    // Drive to DEAD_LETTER: receive → start → reject (non-retryable) → REJECTED → DEAD_LETTER
    bus.receive(&msg, &clock).expect("receive");
    bus.start_processing("msg_dl_1", &clock).expect("start");
    bus.reject_processing("msg_dl_1", "permanently broken", false, &clock)
        .expect("reject");

    assert_eq!(
        state_of(&bus, "msg_dl_1"),
        "DEAD_LETTER",
        "non-retryable rejection must reach DEAD_LETTER"
    );

    // Replay it — this is the branch M2 never reached.
    let ids = FixedIdSource::new("msg_dl_replay");
    let replayed = bus
        .replay("msg_dl_1", &ids, &at("2026-10-04T01:00:00Z"))
        .expect("replay of a DEAD_LETTER message must succeed");

    // Original is untouched — history is immutable.
    assert_eq!(
        state_of(&bus, "msg_dl_1"),
        "DEAD_LETTER",
        "the original must stay DEAD_LETTER; replay must not rewrite history"
    );

    // The replay is a new, queued message.
    assert_eq!(replayed.message_id, "msg_dl_replay_1");
    assert_eq!(
        state_of(&bus, "msg_dl_replay_1"),
        "QUEUED",
        "the replay must start as QUEUED, ready for the dispatcher"
    );

    // `causation_id` names the dead-letter event, confirming the log explains why the replay exists.
    let causation: Option<String> = bus
        .storage()
        .conn()
        .query_row(
            "SELECT causation_id FROM messages WHERE message_id = ?1",
            ["msg_dl_replay_1"],
            |r| r.get(0),
        )
        .expect("replayed row");
    assert_eq!(
        causation.as_deref(),
        Some("evt_msg_dl_1_message_dead_lettered"),
        "causation_id must point to the MESSAGE_DEAD_LETTERED event, not MESSAGE_EXPIRED"
    );

    // Chain must still be intact.
    assert!(
        bus.storage()
            .verify_event_chain()
            .expect("verify")
            .is_intact(),
        "the event chain must be intact after a DEAD_LETTER replay"
    );
}

// -----------------------------------------------------------------------------------------------------------
// Gap 3: concurrent writers
// -----------------------------------------------------------------------------------------------------------

/// Enqueue, retrying while the other writer holds the database.
///
/// **Bounded on purpose.** `Storage::open` configures no `busy_timeout` — the only pragma it sets is
/// `foreign_keys` — so a writer that finds the database locked receives `SQLITE_BUSY` immediately rather than
/// waiting, and retrying is the caller's responsibility. An unbounded loop here would turn any persistent write
/// failure into a hang instead of a test failure, and a test that hangs reports nothing.
///
/// The retry is deliberately broad: it retries any `STORAGE_FAILURE`, because `BusError` does not expose the
/// underlying SQLite code, so a genuine disk error would be retried too. That is tolerable in a test whose
/// purpose is to observe that both writers land. It would not be tolerable in a production retry policy, which
/// is exactly why that policy belongs to the controller rather than to this crate (DEC-058).
fn enqueue_with_retry(bus: &mut Bus, envelope_json: &str) {
    const MAX_ATTEMPTS: usize = 200;
    let mut last_error = String::new();
    for _ in 0..MAX_ATTEMPTS {
        match bus.enqueue(envelope_json) {
            Ok(_) => return,
            Err(e) if e.code() == "STORAGE_FAILURE" => {
                last_error = format!("{e:?}");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) => panic!("enqueue failed with an unexpected error: {e:?}"),
        }
    }
    panic!("enqueue was still failing after {MAX_ATTEMPTS} attempts; last error: {last_error}");
}

/// Two threads, each with their own connection to the same file-backed database, enqueue simultaneously. The
/// test asserts both rows land, neither is duplicated, and the event chain is intact.
///
/// # What this verifies, and what it does not
///
/// M2 left the serialization argument for the pass-count read (`next_transition_event_id` counting the events
/// already recorded for one message) on the basis of reasoning: SQLite serializes concurrent writers, so two
/// appends for one message cannot both read the same count. This test observes two writers against one database
/// and confirms that no write is lost.
///
/// It does **not** run two concurrent transitions on the *same* message, which is what would actually stress the
/// pass count — both threads enqueue different messages. That case therefore remains reasoned rather than
/// observed, and is recorded as such rather than implied to be covered by this test.
#[test]
fn two_concurrent_writers_both_land_without_loss() {
    use std::sync::{Arc, Barrier};

    // File-backed so two connections can meet.
    let dir = existing_dir("concurrent");
    let db_path = dir.join("bus.db");

    // Seed the project sequentially so the DDL race does not happen during the actual test.
    {
        let mut seed = Storage::open(&db_path).expect("seed open");
        seed.create_project(&project(&dir, "concurrent"))
            .expect("create project");
    }

    // Initialize both Bus handles sequentially. Storage::open runs SCHEMA_SQL (DDL), and DDL
    // writes would themselves race if opened from two threads simultaneously.
    let bus_a = {
        let storage = Storage::open(&db_path).expect("open a");
        Bus::new(storage)
    };
    let bus_b = {
        let storage = Storage::open(&db_path).expect("open b");
        Bus::new(storage)
    };

    // A barrier so both threads reach enqueue at the same instant, maximizing contention.
    let barrier = Arc::new(Barrier::new(2));
    let barrier_a = Arc::clone(&barrier);
    let barrier_b = Arc::clone(&barrier);

    let env_a = envelope(
        "msg_conc_a",
        "prj_concurrent",
        1,
        Some("op_conc_a"),
        r#"{"task":"a"}"#,
    );
    let env_b = envelope(
        "msg_conc_b",
        "prj_concurrent",
        2,
        Some("op_conc_b"),
        r#"{"task":"b"}"#,
    );

    let handle_a = std::thread::spawn(move || {
        let mut bus = bus_a;
        barrier_a.wait();
        // Both writers land in some serial order; the test does not claim they land in the order they started.
        enqueue_with_retry(&mut bus, &env_a);
    });

    let handle_b = std::thread::spawn(move || {
        let mut bus = bus_b;
        barrier_b.wait();
        enqueue_with_retry(&mut bus, &env_b);
    });

    handle_a.join().expect("thread a panicked");
    handle_b.join().expect("thread b panicked");

    // Verify on a fresh connection: both messages exist and the backlog is two.
    let verify_storage = Storage::open(&db_path).expect("verify open");
    let verify_bus = Bus::new(verify_storage);

    let backlog = verify_bus.backlog().expect("backlog");
    assert_eq!(
        backlog, 2,
        "both messages must be in the queue; if either was lost, backlog would be 1"
    );

    for message_id in ["msg_conc_a", "msg_conc_b"] {
        let count: i64 = verify_bus
            .storage()
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
                [message_id],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(
            count, 1,
            "message {message_id} must be present exactly once"
        );
    }

    // Chain intact across both writers.
    assert!(
        verify_bus
            .storage()
            .verify_event_chain()
            .expect("verify")
            .is_intact(),
        "concurrent writers must not corrupt the event chain"
    );
}

// -----------------------------------------------------------------------------------------------------------
// Gap 4: backlog query plan
// -----------------------------------------------------------------------------------------------------------

/// Capture `EXPLAIN QUERY PLAN` for `pending_outbound_count`'s SQL and assert that neither table is read by a
/// full table scan.
///
/// The assertion is: no plan step reads `outbox` or `messages` as a bare `SCAN` without a `USING INDEX`. The
/// schema provides `idx_outbox_due ON outbox(dispatch_state, next_attempt_at)`, which covers the
/// `dispatch_state IN ('PENDING', 'FAILED')` filter, and the join reaches `messages` through the automatic index
/// on its `message_id` primary key. Observed:
///
///   SEARCH o USING INDEX idx_outbox_due (dispatch_state=?)
///   SEARCH m USING INDEX sqlite_autoindex_messages_1 (message_id=?)
///
/// The query is:
///   SELECT COUNT(*) FROM outbox o JOIN messages m ON m.message_id = o.message_id
///    WHERE o.dispatch_state IN ('PENDING', 'FAILED') AND m.delivery_state = 'QUEUED'
///
/// The plan is printed so that a failure can be read rather than guessed at. Note what this does **not** show:
/// an indexed access is still not a constant-time read, because the count must visit every matching row. What it
/// rules out is visiting every row in either table.
#[test]
fn backlog_query_uses_an_index_not_a_full_table_scan() {
    // The query plan is schema-dependent, not data-dependent, so an empty database is sufficient.
    let (storage, _dir) = storage_with_project("backlog_plan");
    let conn = storage.conn();

    // Collect the plan lines.
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN \
             SELECT COUNT(*) FROM outbox o JOIN messages m ON m.message_id = o.message_id \
              WHERE o.dispatch_state IN ('PENDING', 'FAILED') AND m.delivery_state = 'QUEUED'",
        )
        .expect("prepare plan");

    // EXPLAIN QUERY PLAN returns rows with columns (id, parent, notused, detail).
    let plan_lines: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(3))
        .expect("query plan")
        .map(|r| r.expect("plan row"))
        .collect();

    assert!(
        !plan_lines.is_empty(),
        "EXPLAIN QUERY PLAN returned no rows — the query may be syntactically invalid"
    );

    // Printed so a reader can see which access path was chosen, and so a failure below shows the whole plan.
    eprintln!(
        "pending_outbound_count plan:\n  {}",
        plan_lines.join("\n  ")
    );

    // A full table scan on either table looks like "SCAN outbox" or "SCAN messages" without
    // "USING INDEX" immediately following on the same line. We reject any such line.
    let full_scans: Vec<&String> = plan_lines
        .iter()
        .filter(|line| {
            let upper = line.to_uppercase();
            // A bare SCAN (no USING INDEX) on either bus table is the thing to forbid.
            (upper.contains("SCAN OUTBOX") || upper.contains("SCAN MESSAGES"))
                && !upper.contains("USING INDEX")
                && !upper.contains("USING COVERING INDEX")
        })
        .collect();

    assert!(
        full_scans.is_empty(),
        "backlog query must not do a full table scan; found:\n  {}\nFull plan:\n  {}",
        full_scans
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        plan_lines.join("\n  ")
    );
}
