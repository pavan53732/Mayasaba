//! Bus-level tests for explicit, controller-invoked replay.
//!
//! The load-bearing properties are that a replay is a **new message** rather than a reset of the old one, that it
//! carries the original's terminal event as its cause so the log says why it exists, and that a message which has
//! not finished failing cannot be replayed at all. Each is checked by reading the tables.

mod common;

use common::{envelope, envelope_at, storage_with_project, ScriptedTransport};
use mayasaba_bus::{BackoffPolicy, Bus, DispatchPolicy, FixedClock, FixedIdSource, TransportError};

// -----------------------------------------------------------------------------------------------------------

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

fn count(bus: &Bus, sql: &str, param: &str) -> i64 {
    bus.storage()
        .conn()
        .query_row(sql, [param], |r| r.get(0))
        .expect("count")
}

/// A bus whose attempt budget is two, so a message can be driven to `EXPIRED` in three passes.
///
/// The budget is configuration rather than a locked constant (DEC-058), which is what makes a short one possible
/// here: the declared edge is the same edge at any budget.
fn bus_with_project(tag: &str) -> (Bus, std::path::PathBuf) {
    let (storage, dir) = storage_with_project(tag);
    (
        Bus::with_policy(
            storage,
            DispatchPolicy {
                max_attempts: 2,
                ..DispatchPolicy::default()
            },
            BackoffPolicy::default(),
        ),
        dir,
    )
}

/// Drive a message to `EXPIRED` through the declared `QUEUED -> EXPIRED` edge, so there is something to replay.
fn expire(bus: &mut Bus, message_id: &str) {
    expire_from(bus, message_id, "2026-10-04T00:00");
}

/// The same, with the passes placed after a given minute.
///
/// A message is only due once its `next_attempt_at` has passed, and a replay's first due time comes from its own
/// `created_at` - so a replay cannot be expired by passes that ran before it existed.
fn expire_from(bus: &mut Bus, message_id: &str, minute: &str) {
    let mut transport = ScriptedTransport::always(TransportError::Unavailable(
        "the adapter is not running".to_string(),
    ));
    // Two attempts, then a pass that finds the budget spent and terminates the message.
    for second in [10, 20, 30] {
        bus.dispatch_due(&at(&format!("{minute}:{second:02}Z")), &mut transport)
            .expect("pass");
    }
    assert_eq!(state_of(bus, message_id), "EXPIRED");
}

#[test]
fn replaying_an_expired_message_creates_a_new_one_that_names_its_cause() {
    let (mut bus, _dir) = bus_with_project("replay");
    bus.enqueue(&envelope(
        "msg_rp_1",
        "prj_replay",
        1,
        Some("op_rp_1"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");
    expire(&mut bus, "msg_rp_1");

    let ids = FixedIdSource::new("msg_rp_replay");
    let replayed = bus
        .replay("msg_rp_1", &ids, &at("2026-10-04T01:00:00Z"))
        .expect("replay");

    assert_eq!(replayed.message_id, "msg_rp_replay_1");
    // A new message, not a reset: the original keeps its identity and its terminal state, because reusing them
    // would rewrite history rather than add to it.
    assert_eq!(state_of(&bus, "msg_rp_1"), "EXPIRED");
    assert_eq!(state_of(&bus, "msg_rp_replay_1"), "QUEUED");
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
            "msg_rp_1"
        ),
        1
    );

    // It carries the original's terminal event as its cause, so the log says why it exists rather than leaving it
    // looking like an unrelated message that happened to be queued.
    let (causation, sequence): (Option<String>, i64) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT causation_id, sequence FROM messages WHERE message_id = ?1",
            ["msg_rp_replay_1"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("replayed row");
    assert_eq!(causation.as_deref(), Some("evt_msg_rp_1_message_expired"));
    // The original still holds its own position, so the replay takes the next free one rather than stealing it.
    assert_eq!(sequence, 2);
}

#[test]
fn a_replayed_message_can_be_dispatched_and_is_a_message_in_its_own_right() {
    let (mut bus, _dir) = bus_with_project("replay_dispatch");
    bus.enqueue(&envelope(
        "msg_rp_2",
        "prj_replay_dispatch",
        1,
        Some("op_rp_2"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");
    expire(&mut bus, "msg_rp_2");

    let ids = FixedIdSource::new("msg_rp_replay");
    bus.replay("msg_rp_2", &ids, &at("2026-10-04T01:00:00Z"))
        .expect("replay");

    // The replay is queued and due, so an ordinary pass hands it over - it is not a special kind of message that
    // only a replay path can move.
    let mut transport = ScriptedTransport::always_ok();
    let report = bus
        .dispatch_due(&at("2026-10-04T02:00:00Z"), &mut transport)
        .expect("pass");
    assert_eq!(report.dispatched(), 1);
    assert_eq!(state_of(&bus, "msg_rp_replay_1"), "DISPATCHED");

    // And it was handed over under its own identity, with its own envelope.
    let handed: Vec<String> = transport
        .sent
        .iter()
        .map(|e| {
            mayasaba_protocol::envelope::parse_envelope(e)
                .expect("envelope")
                .message_id()
                .to_string()
        })
        .collect();
    assert_eq!(handed, vec!["msg_rp_replay_1".to_string()]);
}

#[test]
fn replaying_a_message_that_has_not_finished_failing_is_refused() {
    let (mut bus, _dir) = bus_with_project("replay_refused");
    bus.enqueue(&envelope(
        "msg_rp_3",
        "prj_replay_refused",
        1,
        Some("op_rp_3"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");

    // It is QUEUED, so it has not finished failing. The declared machine has no edge from an in-flight state back
    // to CREATED, so there is no transition to express a replay with - and replaying work still in progress would
    // duplicate it.
    let ids = FixedIdSource::new("msg_rp_replay");
    let err = bus
        .replay("msg_rp_3", &ids, &at("2026-10-04T01:00:00Z"))
        .expect_err("only a terminal message can be replayed");
    assert_eq!(err.code(), "SCHEMA_INVALID");
    assert_eq!(state_of(&bus, "msg_rp_3"), "QUEUED");
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
            "msg_rp_replay_1"
        ),
        0,
        "a refused replay must not have created anything"
    );
}

#[test]
fn replaying_a_message_that_does_not_exist_is_refused() {
    let (mut bus, _dir) = bus_with_project("replay_missing");
    let ids = FixedIdSource::new("msg_rp_replay");
    let err = bus
        .replay("msg_never_existed", &ids, &at("2026-10-04T01:00:00Z"))
        .expect_err("a message that does not exist cannot be replayed");
    // Nothing went wrong with the database, so this is not STORAGE_FAILURE: the request named something the
    // durable record does not hold.
    assert_eq!(err.code(), "SCHEMA_INVALID");
}

#[test]
fn a_second_replay_while_the_first_is_live_is_deduplicated() {
    let (mut bus, _dir) = bus_with_project("replay_twice");
    bus.enqueue(&envelope(
        "msg_rp_4",
        "prj_replay_twice",
        1,
        Some("op_rp_4"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");
    expire(&mut bus, "msg_rp_4");

    let ids = FixedIdSource::new("msg_rp_replay");
    let first = bus
        .replay("msg_rp_4", &ids, &at("2026-10-04T01:00:00Z"))
        .expect("first replay");
    assert_eq!(first.message_id, "msg_rp_replay_1");
    assert!(!first.deduplicated);

    // A replay carries the original's `operation_id`, because it is the same operation. Idempotency exists to stop
    // one operation being queued twice **while it is still pending**, so the second replay is absorbed onto the
    // first rather than creating a second live attempt at one operation. That is the rule working, not a limit of
    // replay.
    let second = bus
        .replay("msg_rp_4", &ids, &at("2026-10-04T01:01:00Z"))
        .expect("second replay");
    assert!(second.deduplicated);
    assert_eq!(second.message_id, "msg_rp_replay_1");
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
            "msg_rp_replay_2"
        ),
        0,
        "a live claim on the operation absorbed the second replay"
    );

    // Once the first replay has terminated, the claim is gone and a further replay is a genuinely new message.
    expire_from(&mut bus, "msg_rp_replay_1", "2026-10-04T01:00");
    let third = bus
        .replay("msg_rp_4", &ids, &at("2026-10-04T02:00:00Z"))
        .expect("third replay");
    assert!(!third.deduplicated);
    assert_ne!(third.message_id, "msg_rp_replay_1");
    assert_eq!(state_of(&bus, &third.message_id), "QUEUED");
    // The original is untouched throughout: three replays never rewrite the message they replay.
    assert_eq!(state_of(&bus, "msg_rp_4"), "EXPIRED");
}
#[test]
fn a_replayed_message_carries_a_fresh_created_at() {
    let (mut bus, _dir) = bus_with_project("replay_time");
    // The original was created long before it expired, and the outbox's first due time is derived from this.
    bus.enqueue(&envelope_at(
        "msg_rp_5",
        "prj_replay_time",
        1,
        Some("op_rp_5"),
        r#"{"task":"a"}"#,
        "2026-01-01T00:00:00Z",
    ))
    .expect("enqueue");
    expire(&mut bus, "msg_rp_5");

    bus.replay(
        "msg_rp_5",
        &FixedIdSource::new("msg_rp_replay"),
        &at("2026-10-04T01:00:00Z"),
    )
    .expect("replay");
    let created: String = bus
        .storage()
        .conn()
        .query_row(
            "SELECT created_at FROM messages WHERE message_id = ?1",
            ["msg_rp_replay_1"],
            |r| r.get(0),
        )
        .expect("row");
    // The replay is a new event in the log, and DEC-034's chain records when things happened - not when the thing
    // they retry happened.
    assert_eq!(created, "2026-10-04T01:00:00Z");
}
