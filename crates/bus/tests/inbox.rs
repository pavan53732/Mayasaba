//! Bus-level tests for the inbound path: the durable inbox, the receipt, and the processing spine.
//!
//! Two properties carry the weight here. **Persistence precedes acknowledgement**, so a crash between them
//! leaves a message that has been seen rather than an acknowledgement of a message nothing recorded. And **a
//! redelivery is answered from the durable inbox**, so the same message arriving twice does not become two
//! side effects. Both are checked by reading the tables and counting what was written, because "the call
//! returned Ok" is exactly the claim a duplicate would also satisfy.

mod common;

use common::{envelope, envelope_at, storage_with_project};
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
