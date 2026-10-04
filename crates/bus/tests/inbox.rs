//! Bus-level tests for the inbound path: the durable inbox, the receipt, and the processing spine.
//!
//! Two properties carry the weight here. **Persistence precedes acknowledgement**, so a crash between them
//! leaves a message that has been seen rather than an acknowledgement of a message nothing recorded. And **a
//! redelivery is answered from the durable inbox**, so the same message arriving twice does not become two
//! side effects. Both are checked by reading the tables and counting what was written, because "the call
//! returned Ok" is exactly the claim a duplicate would also satisfy.

mod common;

use common::{envelope, envelope_at, storage_with_project, ScriptedTransport};
use mayasaba_bus::{Bus, BusError, FixedClock};

fn bus_with_project(tag: &str) -> (Bus, std::path::PathBuf) {
    let (storage, dir) = storage_with_project(tag);
    (Bus::new(storage), dir)
}

fn at(stamp: &str) -> FixedClock {
    FixedClock::new(stamp)
}

fn state_of(bus: &Bus, message_id: &str) -> String {
    bus.storage()
        .conn()
        .query_row(
            "SELECT delivery_state FROM messages WHERE message_id = ?1",
            [message_id],
            |row| row.get(0),
        )
        .expect("delivery_state")
}

fn count(bus: &Bus, sql: &str, message_id: &str) -> i64 {
    bus.storage()
        .conn()
        .query_row(sql, [message_id], |row| row.get(0))
        .expect("count")
}

/// The message's lifecycle events in the order they were appended.
///
/// Ordered by `rowid` rather than by `sequence`, because the `events` table declares no uniqueness on
/// `sequence` and several lifecycle events of one message can share a position.
fn events_of(bus: &Bus, message_id: &str) -> Vec<String> {
    let conn = bus.storage().conn();
    let mut stmt = conn
        .prepare("SELECT event_type FROM events WHERE payload_json LIKE ?1 ORDER BY rowid")
        .expect("prepare");
    let like = format!("%\"{message_id}\"%");
    let rows = stmt
        .query_map([like], |row| row.get::<_, String>(0))
        .expect("query");
    rows.map(|r| r.expect("event")).collect()
}

fn payload_of_last_event(bus: &Bus, message_id: &str) -> String {
    let conn = bus.storage().conn();
    let like = format!("%\"{message_id}\"%");
    conn.query_row(
        "SELECT payload_json FROM events WHERE payload_json LIKE ?1 ORDER BY rowid DESC LIMIT 1",
        [like],
        |row| row.get(0),
    )
    .expect("last payload")
}

// -----------------------------------------------------------------------------------------------------------
// Receiving
// -----------------------------------------------------------------------------------------------------------

#[test]
fn a_receive_persists_the_inbox_row_and_then_acknowledges_receipt() {
    let (mut bus, _dir) = bus_with_project("recv_basic");
    let envelope = envelope(
        "msg_in_1",
        "prj_recv_basic",
        1,
        Some("op_in_1"),
        r#"{"task":"a"}"#,
    );

    let received = bus
        .receive(&envelope, &at("2026-10-04T00:00:10Z"))
        .expect("receive");

    assert_eq!(received.message_id, "msg_in_1");
    assert!(!received.duplicate, "a first delivery is not a duplicate");
    assert_eq!(received.processing_state, "ACKED");
    assert_eq!(received.terminal_event_id, None);

    // The acknowledgement is a receipt and not a success (AGENTS.md section 7), so the message is ACKED and
    // not PROCESSED.
    assert_eq!(state_of(&bus, "msg_in_1"), "ACKED");

    let row = bus
        .storage()
        .inbox_row("msg_in_1")
        .expect("inbox")
        .expect("row");
    assert_eq!(row.processing_state, "ACKED");
    // Persist before acknowledge, and both before any side effect: the two stamps are the same instant here
    // because one transaction wrote them, and `persisted_at` is never later than `acked_at`.
    assert_eq!(row.persisted_at, "2026-10-04T00:00:10Z");
    assert_eq!(row.acked_at.as_deref(), Some("2026-10-04T00:00:10Z"));

    let receipt = bus
        .storage()
        .receipt_row("msg_in_1")
        .expect("receipt")
        .expect("row");
    assert_eq!(receipt.receipt_state, "ACKED");
    assert_eq!(
        receipt.acknowledged_at.as_deref(),
        Some("2026-10-04T00:00:10Z")
    );
    assert_eq!(receipt.receipt_id, "rcpt_msg_in_1");
}

#[test]
fn a_receive_appends_both_declared_transitions_and_no_others() {
    let (mut bus, _dir) = bus_with_project("recv_events");
    let envelope = envelope(
        "msg_in_2",
        "prj_recv_events",
        1,
        Some("op_in_2"),
        r#"{"task":"a"}"#,
    );

    bus.receive(&envelope, &at("2026-10-04T00:00:10Z"))
        .expect("receive");

    // DISPATCHED -> RECEIVED -> ACKED, in that order, with nothing invented between them.
    assert_eq!(
        events_of(&bus, "msg_in_2"),
        vec!["MESSAGE_RECEIVED", "MESSAGE_ACKED"]
    );
}

#[test]
fn a_receive_stores_the_canonical_envelope_rather_than_the_callers_bytes() {
    let (mut bus, _dir) = bus_with_project("recv_canonical");
    let envelope = envelope(
        "msg_in_3",
        "prj_recv_canonical",
        1,
        Some("op_in_3"),
        r#"{"task":"a"}"#,
    );

    bus.receive(&envelope, &at("2026-10-04T00:00:10Z"))
        .expect("receive");

    let stored: String = bus
        .storage()
        .conn()
        .query_row(
            "SELECT envelope_json FROM messages WHERE message_id = ?1",
            ["msg_in_3"],
            |row| row.get(0),
        )
        .expect("envelope_json");
    let reparsed =
        mayasaba_protocol::envelope::parse_envelope(&stored).expect("stored envelope is valid");
    assert_eq!(reparsed.message_id(), "msg_in_3");
    assert_eq!(reparsed.payload_json_text(), r#"{"task":"a"}"#);
}

// -----------------------------------------------------------------------------------------------------------
// Duplicate delivery
// -----------------------------------------------------------------------------------------------------------

#[test]
fn a_redelivery_returns_the_prior_state_and_writes_nothing_at_all() {
    let (mut bus, _dir) = bus_with_project("recv_dup");
    let envelope = envelope(
        "msg_in_4",
        "prj_recv_dup",
        1,
        Some("op_in_4"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("first receive");
    let again = bus
        .receive(&envelope, &clock)
        .expect("redelivery is not an error");

    assert!(
        again.duplicate,
        "the second delivery must be reported as one"
    );
    assert_eq!(again.processing_state, "ACKED");

    // Nothing was written a second time: not an event, not an inbox row, not a receipt.
    assert_eq!(
        events_of(&bus, "msg_in_4"),
        vec!["MESSAGE_RECEIVED", "MESSAGE_ACKED"],
        "a redelivery must not append events"
    );
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM inbox WHERE message_id = ?1",
            "msg_in_4"
        ),
        1
    );
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM message_receipts WHERE message_id = ?1",
            "msg_in_4"
        ),
        1
    );
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
            "msg_in_4"
        ),
        1
    );
}

#[test]
fn a_redelivery_after_processing_reports_the_terminal_state_not_the_acknowledgement() {
    let (mut bus, _dir) = bus_with_project("recv_dup_terminal");
    let envelope = envelope(
        "msg_in_5",
        "prj_recv_dup_terminal",
        1,
        Some("op_in_5"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_in_5", &clock).expect("start");
    bus.complete_processing("msg_in_5", &clock)
        .expect("complete");

    let again = bus.receive(&envelope, &clock).expect("redelivery");
    assert!(again.duplicate);
    assert_eq!(
        again.processing_state, "PROCESSED",
        "a redelivery must report where the message actually got to"
    );
    assert_eq!(
        again.terminal_event_id.as_deref(),
        Some("evt_msg_in_5_action_completed"),
        "the prior outcome is the terminal event, which is what makes it an outcome rather than a state name"
    );
}

#[test]
fn a_receive_for_a_project_that_does_not_exist_is_refused() {
    let (mut bus, _dir) = bus_with_project("recv_unknown_project");
    let envelope = envelope(
        "msg_in_6",
        "prj_nowhere",
        1,
        Some("op_in_6"),
        r#"{"task":"a"}"#,
    );

    let err = bus
        .receive(&envelope, &at("2026-10-04T00:00:10Z"))
        .expect_err("an unknown project must be refused");
    assert_eq!(err.code(), "PROJECT_MISMATCH");
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM messages WHERE message_id = ?1",
            "msg_in_6"
        ),
        0,
        "a refused receive must not leave a message row behind"
    );
}

#[test]
fn a_receive_reusing_a_known_message_id_is_a_duplicate_conflict() {
    let (mut bus, _dir) = bus_with_project("recv_dup_id");
    // An outbound message takes the id first, at position 1.
    bus.enqueue(&envelope(
        "msg_both",
        "prj_recv_dup_id",
        1,
        Some("op_out"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");

    // A different message claims the same identity at a different position, so it is not a redelivery.
    let impostor = envelope(
        "msg_both",
        "prj_recv_dup_id",
        2,
        Some("op_in"),
        r#"{"task":"b"}"#,
    );
    let err = bus
        .receive(&impostor, &at("2026-10-04T00:00:10Z"))
        .expect_err("one id cannot name two messages");
    assert_eq!(err.code(), "DUPLICATE_CONFLICT");
}

#[test]
fn a_receive_reusing_an_ordering_position_is_a_duplicate_conflict() {
    let (mut bus, _dir) = bus_with_project("recv_dup_position");
    bus.enqueue(&envelope(
        "msg_pos_a",
        "prj_recv_dup_position",
        1,
        Some("op_out"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");

    // Both directions share one ordering space per channel, so position 1 is taken.
    let clashing = envelope(
        "msg_pos_b",
        "prj_recv_dup_position",
        1,
        Some("op_in"),
        r#"{"task":"b"}"#,
    );
    let err = bus
        .receive(&clashing, &at("2026-10-04T00:00:10Z"))
        .expect_err("one position cannot hold two messages");
    assert_eq!(err.code(), "DUPLICATE_CONFLICT");
}

// -----------------------------------------------------------------------------------------------------------
// Processing
// -----------------------------------------------------------------------------------------------------------

#[test]
fn processing_advances_through_the_declared_edges() {
    let (mut bus, _dir) = bus_with_project("proc_ok");
    let envelope = envelope(
        "msg_p_1",
        "prj_proc_ok",
        1,
        Some("op_p_1"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_p_1", &clock).expect("start");
    assert_eq!(state_of(&bus, "msg_p_1"), "PROCESSING");
    assert_eq!(
        bus.storage()
            .inbox_row("msg_p_1")
            .expect("inbox")
            .expect("row")
            .processing_state,
        "PROCESSING"
    );

    bus.complete_processing("msg_p_1", &clock)
        .expect("complete");
    assert_eq!(state_of(&bus, "msg_p_1"), "PROCESSED");
    assert_eq!(
        events_of(&bus, "msg_p_1"),
        vec![
            "MESSAGE_RECEIVED",
            "MESSAGE_ACKED",
            "ACTION_STARTED",
            "ACTION_COMPLETED"
        ]
    );
    let row = bus
        .storage()
        .inbox_row("msg_p_1")
        .expect("inbox")
        .expect("row");
    assert_eq!(row.processing_state, "PROCESSED");
    assert_eq!(
        row.terminal_event_id.as_deref(),
        Some("evt_msg_p_1_action_completed")
    );
    // A successful completion leaves the receipt acknowledged: the message was received and then processed,
    // and neither fact replaces the other.
    assert_eq!(
        bus.storage()
            .receipt_row("msg_p_1")
            .expect("receipt")
            .expect("row")
            .receipt_state,
        "ACKED"
    );
}

#[test]
fn a_retryable_refusal_advances_to_retrying_and_nacks_the_receipt() {
    let (mut bus, _dir) = bus_with_project("proc_retry");
    let envelope = envelope(
        "msg_p_2",
        "prj_proc_retry",
        1,
        Some("op_p_2"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_p_2", &clock).expect("start");
    bus.reject_processing("msg_p_2", "the adapter was busy", true, &clock)
        .expect("reject");

    assert_eq!(state_of(&bus, "msg_p_2"), "RETRYING");
    let row = bus
        .storage()
        .inbox_row("msg_p_2")
        .expect("inbox")
        .expect("row");
    assert_eq!(row.processing_state, "RETRYING");
    assert_eq!(
        row.terminal_event_id, None,
        "a retryable refusal is not terminal"
    );
    assert_eq!(
        bus.storage()
            .receipt_row("msg_p_2")
            .expect("receipt")
            .expect("row")
            .receipt_state,
        "NACKED"
    );
    // The reason lives in the event log, which is the durable record of why a decision was taken; the receipt
    // table has no column for it and the table is fixed.
    let payload = payload_of_last_event(&bus, "msg_p_2");
    assert!(
        payload.contains("the adapter was busy"),
        "reason missing: {payload}"
    );
    assert!(
        payload.contains("\"retryable\":true"),
        "retryability missing: {payload}"
    );
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM dead_letters WHERE message_id = ?1",
            "msg_p_2"
        ),
        0,
        "a retryable refusal must not dead-letter the message"
    );
}

#[test]
fn a_terminal_refusal_reaches_dead_letter_through_both_declared_edges() {
    let (mut bus, _dir) = bus_with_project("proc_dead");
    let envelope = envelope(
        "msg_p_3",
        "prj_proc_dead",
        1,
        Some("op_p_3"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_p_3", &clock).expect("start");
    bus.reject_processing(
        "msg_p_3",
        "the payload does not match the brief",
        false,
        &clock,
    )
    .expect("reject");

    // PROCESSING -> REJECTED -> DEAD_LETTER, not a jump to the end.
    assert_eq!(state_of(&bus, "msg_p_3"), "DEAD_LETTER");
    assert_eq!(
        events_of(&bus, "msg_p_3"),
        vec![
            "MESSAGE_RECEIVED",
            "MESSAGE_ACKED",
            "ACTION_STARTED",
            "ACTION_FAILED",
            "MESSAGE_DEAD_LETTERED"
        ]
    );
    let row = bus
        .storage()
        .inbox_row("msg_p_3")
        .expect("inbox")
        .expect("row");
    assert_eq!(row.processing_state, "DEAD_LETTER");
    assert_eq!(
        row.terminal_event_id.as_deref(),
        Some("evt_msg_p_3_message_dead_lettered")
    );

    let (final_error, attempts): (String, i64) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT final_error_json, attempts FROM dead_letters WHERE message_id = ?1",
            ["msg_p_3"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("dead letter");
    assert_eq!(
        final_error, r#"{"code":"PROCESS_FAILED","detail":"the payload does not match the brief"}"#,
        "the dead letter must say why, under the registry's own code for a processing failure"
    );
    // The delivery attempt that brought it here; a rejected message has been delivered exactly once, because a
    // redelivery would have been answered from the inbox rather than processed again.
    assert_eq!(attempts, 1);
}

#[test]
fn processing_a_message_that_was_never_received_is_refused_and_writes_nothing() {
    let (mut bus, _dir) = bus_with_project("proc_missing");
    let err = bus
        .start_processing("msg_absent", &at("2026-10-04T00:00:10Z"))
        .expect_err("a message that was never received cannot be processed");
    // The stored row does not satisfy what the operation requires of it, which is the schema-invalid condition.
    assert_eq!(err.code(), "SCHEMA_INVALID");
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM events WHERE payload_json LIKE ?1",
            "%msg_absent%"
        ),
        0
    );
}

#[test]
fn a_message_cannot_be_processed_twice() {
    let (mut bus, _dir) = bus_with_project("proc_twice");
    let envelope = envelope(
        "msg_p_4",
        "prj_proc_twice",
        1,
        Some("op_p_4"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_p_4", &clock).expect("start");
    bus.complete_processing("msg_p_4", &clock)
        .expect("complete");

    let err = bus
        .start_processing("msg_p_4", &clock)
        .expect_err("a processed message cannot be started again");
    assert_eq!(err.code(), "SCHEMA_INVALID");
    assert_eq!(state_of(&bus, "msg_p_4"), "PROCESSED");
    assert_eq!(
        events_of(&bus, "msg_p_4").len(),
        4,
        "the refused second start must not have appended anything"
    );
}

#[test]
fn a_refused_transition_names_the_state_it_found() {
    let (mut bus, _dir) = bus_with_project("proc_names_state");
    let envelope = envelope_at(
        "msg_p_5",
        "prj_proc_names_state",
        1,
        Some("op_p_5"),
        r#"{"task":"a"}"#,
        "2026-10-04T00:00:05Z",
    );
    let clock = at("2026-10-04T00:00:10Z");
    bus.receive(&envelope, &clock).expect("receive");

    // Completing before starting is a refusal, and the message says which state it found rather than only that
    // zero rows changed.
    let err = bus
        .complete_processing("msg_p_5", &clock)
        .expect_err("refused");
    match err {
        BusError::Malformed { column, detail } => {
            assert_eq!(column, "messages.delivery_state");
            assert!(detail.contains("requires PROCESSING"), "{detail}");
            assert!(detail.contains("found ACKED"), "{detail}");
        }
        other => panic!("expected a malformed-state refusal, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------------------------------------
// Requeue: the exit RETRYING was missing
// -----------------------------------------------------------------------------------------------------------

#[test]
fn a_retryably_refused_message_can_be_requeued_and_processed_again() {
    let (mut bus, _dir) = bus_with_project("requeue");
    let envelope = envelope(
        "msg_r_1",
        "prj_requeue",
        1,
        Some("op_r_1"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_r_1", &clock).expect("start");
    bus.reject_processing("msg_r_1", "the adapter was busy", true, &clock)
        .expect("reject");
    assert_eq!(state_of(&bus, "msg_r_1"), "RETRYING");

    // RETRYING -> QUEUED, the edge that stops a refused message waiting forever.
    bus.requeue("msg_r_1", &clock).expect("requeue");
    assert_eq!(state_of(&bus, "msg_r_1"), "QUEUED");
    let (dispatch_state, due): (String, String) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT dispatch_state, next_attempt_at FROM outbox WHERE message_id = ?1",
            ["msg_r_1"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("outbox");
    assert_eq!(dispatch_state, "PENDING");
    // Immediately due: no processing-attempt count exists to derive a delay from, so requeueing is a decision
    // the caller takes rather than a timer the bus invents.
    assert_eq!(due, "2026-10-04T00:00:10Z");

    // Round again: dispatched, received, acknowledged, processed.
    let mut transport = ScriptedTransport::always_ok();
    bus.dispatch_due(&clock, &mut transport).expect("dispatch");
    assert_eq!(state_of(&bus, "msg_r_1"), "DISPATCHED");

    let again = bus.receive(&envelope, &clock).expect("redelivery");
    assert!(
        !again.duplicate,
        "a message that was requeued is being delivered again, not duplicated"
    );
    assert_eq!(again.processing_state, "ACKED");
    bus.start_processing("msg_r_1", &clock)
        .expect("start again");
    bus.complete_processing("msg_r_1", &clock)
        .expect("complete");
    assert_eq!(state_of(&bus, "msg_r_1"), "PROCESSED");

    // The second pass added events but no second identity: one inbox row, one receipt.
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM inbox WHERE message_id = ?1",
            "msg_r_1"
        ),
        1
    );
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM message_receipts WHERE message_id = ?1",
            "msg_r_1"
        ),
        1
    );
    // Every declared transition the machine performed, in order, with the repeated ones recorded rather than
    // dropped - which is what the pass-suffixed event id exists to allow.
    assert_eq!(
        events_of(&bus, "msg_r_1"),
        vec![
            "MESSAGE_RECEIVED",
            "MESSAGE_ACKED",
            "ACTION_STARTED",
            "ACTION_FAILED",
            "MESSAGE_QUEUED",
            "MESSAGE_DISPATCHED",
            "MESSAGE_RECEIVED",
            "MESSAGE_ACKED",
            "ACTION_STARTED",
            "ACTION_COMPLETED"
        ]
    );
}

#[test]
fn a_repeated_transition_gets_its_own_event_id_and_the_first_keeps_the_plain_one() {
    let (mut bus, _dir) = bus_with_project("requeue_ids");
    let envelope = envelope(
        "msg_r_3",
        "prj_requeue_ids",
        1,
        Some("op_r_3"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_r_3", &clock).expect("start");
    bus.reject_processing("msg_r_3", "busy", true, &clock)
        .expect("reject");
    bus.requeue("msg_r_3", &clock).expect("requeue");
    let mut transport = ScriptedTransport::always_ok();
    bus.dispatch_due(&clock, &mut transport).expect("dispatch");
    bus.receive(&envelope, &clock).expect("redelivery");

    let ids: Vec<String> = {
        let conn = bus.storage().conn();
        let mut stmt = conn
            .prepare("SELECT event_id FROM events WHERE payload_json LIKE ?1 ORDER BY rowid")
            .expect("prepare");
        let like = "%\"msg_r_3\"%";
        let rows = stmt
            .query_map([like], |r| r.get::<_, String>(0))
            .expect("query");
        rows.map(|r| r.expect("id")).collect()
    };
    // The first occurrence of each transition keeps the plain id, so nothing already written is renamed; the
    // second is suffixed from the durable count, so it is deterministic rather than random.
    assert!(
        ids.contains(&"evt_msg_r_3_message_received".to_string()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"evt_msg_r_3_message_received_2".to_string()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"evt_msg_r_3_message_acked_2".to_string()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"evt_msg_r_3_message_dispatched".to_string()),
        "{ids:?}"
    );
}

#[test]
fn requeueing_a_message_that_is_not_retrying_is_refused() {
    let (mut bus, _dir) = bus_with_project("requeue_refused");
    let envelope = envelope(
        "msg_r_2",
        "prj_requeue_refused",
        1,
        Some("op_r_2"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");
    bus.receive(&envelope, &clock).expect("receive");

    let err = bus
        .requeue("msg_r_2", &clock)
        .expect_err("only a retrying message can be requeued");
    assert_eq!(err.code(), "SCHEMA_INVALID");
    assert_eq!(state_of(&bus, "msg_r_2"), "ACKED");
    assert_eq!(
        events_of(&bus, "msg_r_2"),
        vec!["MESSAGE_RECEIVED", "MESSAGE_ACKED"],
        "a refused requeue must not have appended anything"
    );
}

#[test]
fn a_message_refused_retryably_and_never_requeued_can_be_expired() {
    let (mut bus, _dir) = bus_with_project("retrying_expire");
    let envelope = envelope(
        "msg_x_1",
        "prj_retrying_expire",
        1,
        Some("op_x_1"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_x_1", &clock).expect("start");
    bus.reject_processing("msg_x_1", "the adapter was busy", true, &clock)
        .expect("reject");
    assert_eq!(state_of(&bus, "msg_x_1"), "RETRYING");

    // RETRYING -> EXPIRED, the last declared exit the bus was missing.
    bus.expire_retrying("msg_x_1", &clock).expect("expire");
    assert_eq!(state_of(&bus, "msg_x_1"), "EXPIRED");
    let row = bus
        .storage()
        .inbox_row("msg_x_1")
        .expect("inbox")
        .expect("row");
    assert_eq!(row.processing_state, "EXPIRED");
    assert_eq!(
        row.terminal_event_id.as_deref(),
        Some("evt_msg_x_1_message_expired")
    );
    assert_eq!(
        events_of(&bus, "msg_x_1"),
        vec![
            "MESSAGE_RECEIVED",
            "MESSAGE_ACKED",
            "ACTION_STARTED",
            "ACTION_FAILED",
            "MESSAGE_EXPIRED"
        ]
    );
    // No queue entry exists, and the expiry must not create one: this message was received, not queued, so there
    // is nothing to abandon. Creating one would leave an abandoned queue entry for a message that never queued.
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM outbox WHERE message_id = ?1",
            "msg_x_1"
        ),
        0
    );

    let (final_error, attempts): (String, i64) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT final_error_json, attempts FROM dead_letters WHERE message_id = ?1",
            ["msg_x_1"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("dead letter");
    // The reason is the budget, not one particular refusal: the refusals themselves are in the event log, and
    // there is no stored processing-attempt count to derive an expiry from.
    assert_eq!(
        final_error,
        r#"{"code":"PROCESS_FAILED","detail":"the processing budget was exhausted"}"#
    );
    assert_eq!(attempts, 1);

    // A redelivery after the expiry is a duplicate, and reports the terminal event.
    let again = bus.receive(&envelope, &clock).expect("redelivery");
    assert!(again.duplicate);
    assert_eq!(again.processing_state, "EXPIRED");
    assert_eq!(
        again.terminal_event_id.as_deref(),
        Some("evt_msg_x_1_message_expired")
    );
}

#[test]
fn expiring_a_message_that_is_not_retrying_is_refused() {
    let (mut bus, _dir) = bus_with_project("expire_refused");
    let envelope = envelope(
        "msg_x_2",
        "prj_expire_refused",
        1,
        Some("op_x_2"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");
    bus.receive(&envelope, &clock).expect("receive");

    let err = bus
        .expire_retrying("msg_x_2", &clock)
        .expect_err("only a retrying message can expire from RETRYING");
    assert_eq!(err.code(), "SCHEMA_INVALID");
    assert_eq!(state_of(&bus, "msg_x_2"), "ACKED");
    assert_eq!(
        count(
            &bus,
            "SELECT COUNT(*) FROM dead_letters WHERE message_id = ?1",
            "msg_x_2"
        ),
        0,
        "a refused expiry must not dead-letter the message"
    );
}

#[test]
fn expiring_a_requeued_message_abandons_its_queue_entry() {
    let (mut bus, _dir) = bus_with_project("expire_requeued");
    let envelope = envelope(
        "msg_x_3",
        "prj_expire_requeued",
        1,
        Some("op_x_3"),
        r#"{"task":"a"}"#,
    );
    let clock = at("2026-10-04T00:00:10Z");

    // Round once so a queue entry exists, then refuse again and expire from RETRYING.
    bus.receive(&envelope, &clock).expect("receive");
    bus.start_processing("msg_x_3", &clock).expect("start");
    bus.reject_processing("msg_x_3", "busy", true, &clock)
        .expect("reject");
    bus.requeue("msg_x_3", &clock).expect("requeue");
    let mut transport = ScriptedTransport::always_ok();
    bus.dispatch_due(&clock, &mut transport).expect("dispatch");
    bus.receive(&envelope, &clock).expect("redelivery");
    bus.start_processing("msg_x_3", &clock)
        .expect("start again");
    bus.reject_processing("msg_x_3", "still busy", true, &clock)
        .expect("reject again");
    bus.expire_retrying("msg_x_3", &clock).expect("expire");

    assert_eq!(state_of(&bus, "msg_x_3"), "EXPIRED");
    // A queue entry exists this time, and the expiry abandons it rather than leaving it claimable: a later pass
    // must not send a message that has already been declared unprocessable.
    let (dispatch_state, due): (String, Option<String>) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT dispatch_state, next_attempt_at FROM outbox WHERE message_id = ?1",
            ["msg_x_3"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("outbox");
    assert_eq!(dispatch_state, "ABANDONED");
    assert_eq!(due, None);

    // The second pass's transitions are recorded under their own ids, which is what the pass suffix is for.
    let ids: Vec<String> = {
        let conn = bus.storage().conn();
        let mut stmt = conn
            .prepare("SELECT event_id FROM events WHERE payload_json LIKE ?1 ORDER BY rowid")
            .expect("prepare");
        let rows = stmt
            .query_map(["%\"msg_x_3\"%"], |r| r.get::<_, String>(0))
            .expect("query");
        rows.map(|r| r.expect("id")).collect()
    };
    assert!(
        ids.contains(&"evt_msg_x_3_action_failed".to_string()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"evt_msg_x_3_action_failed_2".to_string()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"evt_msg_x_3_message_expired".to_string()),
        "{ids:?}"
    );
}

// -----------------------------------------------------------------------------------------------------------
// Gap detection: reported, never refused
// -----------------------------------------------------------------------------------------------------------

#[test]
fn an_arrival_that_skips_a_position_reports_the_gap_and_is_still_accepted() {
    let (mut bus, _dir) = bus_with_project("gap");
    let clock = at("2026-10-04T00:00:10Z");
    // Position 1 is taken by an outbound message, so position 2 is the next the stream may continue at.
    bus.enqueue(&envelope(
        "msg_g_1",
        "prj_gap",
        1,
        Some("op_g_1"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");

    let arriving = envelope("msg_g_3", "prj_gap", 3, Some("op_g_3"), r#"{"task":"c"}"#);
    let received = bus.receive(&arriving, &clock).expect("receive");

    let gap = received.gap.expect("a skipped position must be reported");
    assert_eq!(gap.session_id, "sess_1");
    assert_eq!(gap.channel, "task");
    assert_eq!(
        gap.expected, 2,
        "the position the stream should have continued at"
    );
    assert_eq!(gap.found, 3);
    // Reported, not refused: refusing would discard the arrival that makes the gap visible.
    assert!(!received.duplicate);
    assert_eq!(state_of(&bus, "msg_g_3"), "ACKED");
}

#[test]
fn an_arrival_at_the_next_position_reports_no_gap() {
    let (mut bus, _dir) = bus_with_project("no_gap");
    let clock = at("2026-10-04T00:00:10Z");
    bus.enqueue(&envelope(
        "msg_n_1",
        "prj_no_gap",
        1,
        Some("op_n_1"),
        r#"{"task":"a"}"#,
    ))
    .expect("enqueue");

    let received = bus
        .receive(
            &envelope(
                "msg_n_2",
                "prj_no_gap",
                2,
                Some("op_n_2"),
                r#"{"task":"b"}"#,
            ),
            &clock,
        )
        .expect("receive");
    assert_eq!(received.gap, None, "position 2 follows position 1");
}

#[test]
fn the_first_message_in_a_channel_reports_no_gap() {
    let (mut bus, _dir) = bus_with_project("first_in_channel");
    // Nothing precedes it, so there is nothing to be missing from - a high position is not a gap on its own.
    let received = bus
        .receive(
            &envelope(
                "msg_f_9",
                "prj_first_in_channel",
                9,
                Some("op_f_9"),
                r#"{"task":"z"}"#,
            ),
            &at("2026-10-04T00:00:10Z"),
        )
        .expect("receive");
    assert_eq!(received.gap, None);
    assert_eq!(state_of(&bus, "msg_f_9"), "ACKED");
}

#[test]
fn a_redelivery_reports_no_gap() {
    let (mut bus, _dir) = bus_with_project("gap_dup");
    let clock = at("2026-10-04T00:00:10Z");
    let arriving = envelope(
        "msg_g_4",
        "prj_gap_dup",
        1,
        Some("op_g_4"),
        r#"{"task":"a"}"#,
    );
    bus.receive(&arriving, &clock).expect("receive");
    let again = bus.receive(&arriving, &clock).expect("redelivery");
    assert!(again.duplicate);
    assert_eq!(
        again.gap, None,
        "nothing new arrived, so nothing new can be missing"
    );
}
