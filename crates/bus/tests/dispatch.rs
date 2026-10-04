//! Bus-level tests for the dispatch, retry, backoff and dead-letter path.
//!
//! The load-bearing properties are that the decision comes from **stored** state, that a refused handover does
//! **not** advance `messages.delivery_state`, and that an exhausted budget terminates through the declared
//! `QUEUED -> EXPIRED` edge with a dead letter rather than reaching `DEAD_LETTER`, which belongs to a receiver's
//! refusal. Each is checked by reading the tables.

mod common;

use common::{envelope_at, envelope_with_priority, storage_with_project, ScriptedTransport};
use mayasaba_bus::{
    decide, BackoffPolicy, Bus, DispatchDecision, DispatchOutcome, DispatchPolicy, FixedClock,
    TransportError,
};

/// A clock the test moves by constructing a new one, because a pass reads the clock once.
fn at(stamp: &str) -> FixedClock {
    FixedClock::new(stamp)
}

/// An envelope whose `created_at` is the given instant, so the outbox's first due time is known.
fn due_at(message_id: &str, project: &str, created_at: &str) -> String {
    due_with_identity(message_id, project, 1, "op_1", created_at)
}

/// The same, with its own ordering position and operation.
///
/// A batch of messages needs distinct `operation_id`s: `project_id + operation_id` is the idempotency scope, so
/// four messages sharing one operation would be four retries of one message and only one would exist.
fn due_with_identity(
    message_id: &str,
    project: &str,
    sequence: i64,
    operation_id: &str,
    created_at: &str,
) -> String {
    envelope_at(
        message_id,
        project,
        sequence,
        Some(operation_id),
        r#"{"task":"a"}"#,
        created_at,
    )
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

fn outbox_of(bus: &Bus, message_id: &str) -> (String, i64, Option<String>) {
    bus.storage()
        .conn()
        .query_row(
            "SELECT dispatch_state, attempts, next_attempt_at FROM outbox WHERE message_id = ?1",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("outbox row")
}

fn event_types(bus: &Bus, project_id: &str) -> Vec<String> {
    let mut stmt = bus
        .storage()
        .conn()
        .prepare("SELECT event_type FROM events WHERE project_id = ?1 ORDER BY sequence")
        .expect("prepare");
    let rows = stmt.query_map([project_id], |r| r.get(0)).expect("query");
    rows.map(|r| r.expect("row")).collect()
}

// --- The pure decision ---------------------------------------------------------------------------------

#[test]
fn the_decision_is_taken_from_the_recorded_attempt_count_and_nothing_else() {
    let policy = DispatchPolicy {
        max_attempts: 3,
        batch_size: 10,
    };
    // Below the budget: send, numbered from one.
    assert_eq!(
        decide(0, &policy),
        DispatchDecision::Send { attempt_no: 1 },
        "no attempts recorded means this is the first"
    );
    assert_eq!(decide(1, &policy), DispatchDecision::Send { attempt_no: 2 });
    assert_eq!(decide(2, &policy), DispatchDecision::Send { attempt_no: 3 });
    // At the budget: stop.
    assert_eq!(decide(3, &policy), DispatchDecision::Expire { attempts: 3 });
    assert_eq!(
        decide(99, &policy),
        DispatchDecision::Expire { attempts: 99 },
        "a count past the budget is still an expiry, and reports the real count"
    );
    // A count the column's type admits but nothing writes is read as "none yet", because the alternative is a
    // message that can never be sent.
    assert_eq!(
        decide(-4, &policy),
        DispatchDecision::Send { attempt_no: 1 }
    );
}

#[test]
fn backoff_is_deterministic_bounded_and_never_in_the_past() {
    let policy = BackoffPolicy {
        base_seconds: 2,
        multiplier: 2,
        cap_seconds: 10,
    };
    // 2, 4, 8, then the cap.
    assert_eq!(policy.delay_seconds(1), 2);
    assert_eq!(policy.delay_seconds(2), 4);
    assert_eq!(policy.delay_seconds(3), 8);
    assert_eq!(policy.delay_seconds(4), 10, "capped");
    assert_eq!(policy.delay_seconds(1000), 10, "and it stays capped");
    // Determinism is the property that matters: the same stored state gives the same answer, twice.
    for attempt in 1..=6 {
        assert_eq!(policy.delay_seconds(attempt), policy.delay_seconds(attempt));
    }
    // A multiplier of one is a constant delay rather than an infinite loop.
    let flat = BackoffPolicy {
        base_seconds: 3,
        multiplier: 1,
        cap_seconds: 60,
    };
    assert_eq!(flat.delay_seconds(5), 3);
    // A hostile policy still cannot produce a delay beyond its own cap or below zero.
    let hostile = BackoffPolicy {
        base_seconds: -5,
        multiplier: -2,
        cap_seconds: 7,
    };
    assert!((0..=7).contains(&hostile.delay_seconds(1)));
    assert!((0..=7).contains(&hostile.delay_seconds(50)));
}

// --- The pass ------------------------------------------------------------------------------------------

#[test]
fn a_due_message_is_handed_over_and_advances_to_dispatched() {
    let (storage, dir) = storage_with_project("dispatch-ok");
    let mut bus = Bus::new(storage);
    bus.enqueue(&due_at("msg_1", "prj_dispatch-ok", "2026-10-04T00:00:05Z"))
        .expect("enqueue");
    let mut transport = ScriptedTransport::always_ok();

    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("pass");

    assert_eq!(report.dispatched(), 1);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.expired(), 0);
    assert_eq!(state_of(&bus, "msg_1"), "DISPATCHED");
    assert_eq!(outbox_of(&bus, "msg_1").0, "DISPATCHED");
    assert_eq!(
        outbox_of(&bus, "msg_1").2,
        None,
        "a dispatched entry is not due again"
    );
    // What was sent is what was stored, byte for byte: a dispatcher that re-serialized could send something the
    // durable record does not describe.
    let stored: String = bus
        .storage()
        .conn()
        .query_row(
            "SELECT envelope_json FROM messages WHERE message_id = 'msg_1'",
            [],
            |r| r.get(0),
        )
        .expect("row");
    assert_eq!(transport.sent, vec![stored]);
    assert_eq!(
        event_types(&bus, "prj_dispatch-ok"),
        vec![
            "PROJECT_CREATED",
            "MESSAGE_PERSISTED",
            "MESSAGE_QUEUED",
            "MESSAGE_DISPATCHED"
        ]
    );
    assert!(bus
        .storage()
        .verify_event_chain()
        .expect("verify")
        .is_intact());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_handover_leaves_the_message_queued_and_records_the_attempt() {
    let (storage, dir) = storage_with_project("dispatch-fail");
    let mut bus = Bus::new(storage);
    bus.enqueue(&due_at(
        "msg_1",
        "prj_dispatch-fail",
        "2026-10-04T00:00:05Z",
    ))
    .expect("enqueue");
    let mut transport = ScriptedTransport::always(TransportError::Unavailable("no adapter".into()));

    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("pass");

    match report.outcomes.as_slice() {
        [DispatchOutcome::Failed {
            attempt_no,
            code,
            retry_in_seconds,
            ..
        }] => {
            assert_eq!(*attempt_no, 1);
            assert_eq!(*code, "ADAPTER_UNAVAILABLE");
            assert_eq!(*retry_in_seconds, 2, "the shipped base delay");
        }
        other => panic!("expected one refusal, got {other:?}"),
    }

    // The accepted constraint: a transport failure never advances delivery_state, and there is no event for it
    // because the machine declares no such transition.
    assert_eq!(
        state_of(&bus, "msg_1"),
        "QUEUED",
        "DISPATCHED means handed over, never tried"
    );
    let (dispatch_state, attempts, next_attempt_at) = outbox_of(&bus, "msg_1");
    assert_eq!(dispatch_state, "FAILED");
    assert_eq!(attempts, 1);
    assert_eq!(next_attempt_at.as_deref(), Some("2026-10-04T00:00:08Z"));
    assert_eq!(
        event_types(&bus, "prj_dispatch-fail"),
        vec!["PROJECT_CREATED", "MESSAGE_PERSISTED", "MESSAGE_QUEUED"],
        "a failed handover is not a transition, so it appends no event"
    );

    // The attempt is recorded durably, which is what the observability requirement asks for.
    let (outcome, error_json, started): (String, String, String) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT outcome, error_json, started_at FROM message_attempts WHERE message_id = 'msg_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("attempt row");
    assert_eq!(outcome, "TRANSPORT_REFUSED");
    assert_eq!(
        error_json,
        r#"{"code":"ADAPTER_UNAVAILABLE","detail":"no adapter"}"#
    );
    assert_eq!(started, "2026-10-04T00:00:06Z");
    assert!(bus
        .storage()
        .verify_event_chain()
        .expect("verify")
        .is_intact());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_message_is_not_due_again_until_its_backoff_has_elapsed() {
    let (storage, dir) = storage_with_project("backoff");
    let mut bus = Bus::new(storage);
    bus.enqueue(&due_at("msg_1", "prj_backoff", "2026-10-04T00:00:05Z"))
        .expect("enqueue");
    let mut transport = ScriptedTransport::always(TransportError::Unavailable("no adapter".into()));

    bus.dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("first pass");
    assert_eq!(transport.attempts(), 1);

    // Still inside the two-second backoff: nothing is due, and nothing is sent.
    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:07Z"), &mut transport)
        .expect("early pass");
    assert!(report.outcomes.is_empty(), "got {report:?}");
    assert_eq!(
        transport.attempts(),
        1,
        "a backoff that sends is not a backoff"
    );

    // At the due instant it is claimed again, and the delay doubles because the attempt count doubled.
    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:08Z"), &mut transport)
        .expect("due pass");
    match report.outcomes.as_slice() {
        [DispatchOutcome::Failed {
            attempt_no,
            retry_in_seconds,
            ..
        }] => {
            assert_eq!(*attempt_no, 2);
            assert_eq!(*retry_in_seconds, 4);
        }
        other => panic!("expected the second attempt, got {other:?}"),
    }
    assert_eq!(
        outbox_of(&bus, "msg_1").2.as_deref(),
        Some("2026-10-04T00:00:12Z")
    );
    assert_eq!(outbox_of(&bus, "msg_1").1, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exhausting_the_budget_expires_the_message_through_the_declared_edge() {
    let (storage, dir) = storage_with_project("exhaust");
    let policy = DispatchPolicy {
        max_attempts: 2,
        batch_size: 10,
    };
    let backoff = BackoffPolicy {
        base_seconds: 1,
        multiplier: 1,
        cap_seconds: 1,
    };
    let mut bus = Bus::with_policy(storage, policy, backoff);
    bus.enqueue(&due_at("msg_1", "prj_exhaust", "2026-10-04T00:00:05Z"))
        .expect("enqueue");
    let mut transport = ScriptedTransport::always(TransportError::Timeout("no answer".into()));

    bus.dispatch_due(&at("2026-10-04T00:00:05Z"), &mut transport)
        .expect("attempt 1");
    assert_eq!(state_of(&bus, "msg_1"), "QUEUED");
    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("attempt 2");
    assert_eq!(state_of(&bus, "msg_1"), "QUEUED", "still not a transition");

    // The third pass finds the budget spent and terminates the message.
    let report2 = bus
        .dispatch_due(&at("2026-10-04T00:00:07Z"), &mut transport)
        .expect("expiry pass");
    assert_eq!(report2.expired(), 1);
    assert_eq!(transport.attempts(), 2, "an expiry is not another send");
    assert_eq!(
        state_of(&bus, "msg_1"),
        "EXPIRED",
        "budget exhaustion terminates through QUEUED -> EXPIRED, which the machine declares"
    );
    assert_eq!(outbox_of(&bus, "msg_1").0, "ABANDONED");
    assert_ne!(
        state_of(&bus, "msg_1"),
        "DEAD_LETTER",
        "DEAD_LETTER is reserved for PROCESSING -> REJECTED -> DEAD_LETTER, a receiver's refusal"
    );

    // The dead letter retains the identity, the final error and the attempt count - the last *real* failure,
    // not the bookkeeping row that discovered the budget was gone.
    let (dead_letter_id, final_error, attempts): (String, String, i64) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT dead_letter_id, final_error_json, attempts FROM dead_letters WHERE message_id = 'msg_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("dead letter");
    assert_eq!(dead_letter_id, "dl_msg_1");
    assert_eq!(attempts, 2);
    assert_eq!(
        final_error,
        r#"{"code":"TIMEOUT","detail":"no answer"}"#,
        "the dead letter must say why the message died, which is the last failure and not the expiry itself"
    );

    assert_eq!(
        event_types(&bus, "prj_exhaust"),
        vec![
            "PROJECT_CREATED",
            "MESSAGE_PERSISTED",
            "MESSAGE_QUEUED",
            "MESSAGE_EXPIRED"
        ],
        "exactly one transition event, and it is the declared one"
    );
    assert!(bus
        .storage()
        .verify_event_chain()
        .expect("verify")
        .is_intact());

    // And it is no longer due, so a later pass does nothing.
    let after = bus
        .dispatch_due(&at("2026-10-04T00:10:00Z"), &mut transport)
        .expect("later pass");
    assert!(after.outcomes.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = report;
}

#[test]
fn one_pass_claims_no_more_than_its_batch_size() {
    let (storage, dir) = storage_with_project("batch");
    let policy = DispatchPolicy {
        max_attempts: 5,
        batch_size: 2,
    };
    let mut bus = Bus::with_policy(storage, policy, BackoffPolicy::default());
    for n in 1..=4 {
        bus.enqueue(&due_with_identity(
            &format!("msg_{n}"),
            "prj_batch",
            n,
            &format!("op_{n}"),
            "2026-10-04T00:00:05Z",
        ))
        .expect("enqueue");
    }
    let mut transport = ScriptedTransport::always_ok();

    let first = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("first pass");
    assert_eq!(first.dispatched(), 2, "batch_size bounds one pass");
    let second = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("second pass");
    assert_eq!(second.dispatched(), 2);
    let third = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("third pass");
    assert!(third.outcomes.is_empty(), "the backlog is worked through");
    assert_eq!(transport.attempts(), 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_due_time_is_normalized_so_an_offset_cannot_misorder_it() {
    // The envelope's `created_at` is `format: date-time`, which admits an offset. The due check compares the
    // outbox column with a UTC clock stamp, so a stamp stored with an offset would compare wrongly - and
    // silently, because the comparison still succeeds.
    let (storage, dir) = storage_with_project("normalize");
    let mut bus = Bus::new(storage);
    bus.enqueue(&due_at(
        "msg_1",
        "prj_normalize",
        "2026-10-04T02:00:05+02:00",
    ))
    .expect("enqueue");

    assert_eq!(
        outbox_of(&bus, "msg_1").2.as_deref(),
        Some("2026-10-04T00:00:05Z"),
        "the same instant, in the one spelling the due check can compare"
    );
    // `messages.created_at` keeps the sender's own spelling, because that column is the record of what the
    // sender said rather than a value the bus compares.
    let sender_stamp: String = bus
        .storage()
        .conn()
        .query_row(
            "SELECT created_at FROM messages WHERE message_id = 'msg_1'",
            [],
            |r| r.get(0),
        )
        .expect("row");
    assert_eq!(sender_stamp, "2026-10-04T02:00:05+02:00");

    // It is due at the normalized instant and not before it.
    let mut transport = ScriptedTransport::always_ok();
    assert!(bus
        .dispatch_due(&at("2026-10-04T00:00:04Z"), &mut transport)
        .expect("early pass")
        .outcomes
        .is_empty());
    assert_eq!(
        bus.dispatch_due(&at("2026-10-04T00:00:05Z"), &mut transport)
            .expect("due pass")
            .dispatched(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_persist_failure_stops_the_pass_rather_than_continuing_to_send() {
    // A dispatcher that cannot write down what it did must stop. Continuing would send messages whose record it
    // cannot keep, which is the one outcome the durable outbox exists to prevent.
    let (storage, dir) = storage_with_project("persist-fail");
    let mut bus = Bus::new(storage);
    bus.enqueue(&due_at("msg_1", "prj_persist-fail", "2026-10-04T00:00:05Z"))
        .expect("enqueue");
    bus.storage()
        .inject_fault_before_insert("message_attempts")
        .expect("trigger");
    let mut transport = ScriptedTransport::always_ok();

    let err = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect_err("the attempt cannot be recorded");
    assert_eq!(err.code(), "STORAGE_FAILURE");
    assert_eq!(
        state_of(&bus, "msg_1"),
        "QUEUED",
        "the transition is rolled back with the attempt"
    );
    assert_eq!(outbox_of(&bus, "msg_1").0, "PENDING");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dispatched_message_is_never_sent_twice_by_a_later_pass() {
    let (storage, dir) = storage_with_project("once");
    let mut bus = Bus::new(storage);
    bus.enqueue(&due_at("msg_1", "prj_once", "2026-10-04T00:00:05Z"))
        .expect("enqueue");
    let mut transport = ScriptedTransport::always_ok();

    bus.dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("first pass");
    let later = bus
        .dispatch_due(&at("2026-10-05T00:00:00Z"), &mut transport)
        .expect("later pass");
    assert!(later.outcomes.is_empty());
    assert_eq!(transport.attempts(), 1);
    assert_eq!(
        event_types(&bus, "prj_once")
            .iter()
            .filter(|t| *t == "MESSAGE_DISPATCHED")
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_transport_failure_vocabulary_is_the_registrys_own_words() {
    // No new code was needed for a failed handover, and TRANSPORT_FAILURE is deliberately not one of them: its
    // registered meaning is the frontend side of the Tauri boundary.
    assert_eq!(
        TransportError::Unavailable("x".into()).code(),
        "ADAPTER_UNAVAILABLE"
    );
    assert_eq!(TransportError::Timeout("x".into()).code(), "TIMEOUT");
    assert_eq!(
        TransportError::ProtocolError("x".into()).code(),
        "ADAPTER_PROTOCOL_ERROR"
    );
    for failure in [
        TransportError::Unavailable("x".into()),
        TransportError::Timeout("x".into()),
        TransportError::ProtocolError("x".into()),
    ] {
        assert_ne!(failure.code(), "TRANSPORT_FAILURE");
        assert!(failure.to_string().starts_with(failure.code()));
    }
}

#[test]
fn the_shipped_defaults_are_the_ones_the_policy_file_declares() {
    // The gate compares these constants with `schemas/mcf-v2/bus-policies.json` mechanically; this test states
    // the same expectation where a reader of the crate will meet it, and fails if a default is changed here
    // without the file changing too.
    let dispatch = DispatchPolicy::default();
    assert_eq!(dispatch.max_attempts, 5);
    assert_eq!(dispatch.batch_size, 32);
    let backoff = BackoffPolicy::default();
    assert_eq!(backoff.base_seconds, 2);
    assert_eq!(backoff.multiplier, 2);
    assert_eq!(backoff.cap_seconds, 300);
}

#[test]
fn a_pass_serves_the_control_lane_before_the_bulk_lane() {
    // The design requires that "emergency control and recovery traffic must not be blocked by bulk model
    // output". All four messages are due at the same instant, so only the lane can decide the order, and the
    // batch is deliberately smaller than the backlog so the assertion is about order and not about capacity.
    let (storage, _dir) = storage_with_project("lane_order");
    let policy = DispatchPolicy {
        max_attempts: 3,
        batch_size: 3,
    };
    let mut bus = Bus::with_policy(storage, policy, BackoffPolicy::default());
    let lanes = [
        ("msg_bulk", 1, "BULK"),
        ("msg_heartbeat", 2, "PROGRESS_HEARTBEAT"),
        ("msg_control", 3, "EMERGENCY_CONTROL"),
        ("msg_recovery", 4, "FAILURE_RECOVERY"),
    ];
    for (message_id, sequence, lane) in lanes {
        bus.enqueue(&envelope_with_priority(
            message_id,
            "prj_lane_order",
            sequence,
            Some(message_id),
            r#"{"task":"a"}"#,
            "2026-10-04T00:00:05Z",
            lane,
        ))
        .expect("enqueue");
    }
    let mut transport = ScriptedTransport::always_ok();

    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:06Z"), &mut transport)
        .expect("dispatch");

    assert_eq!(report.dispatched(), 3, "the batch is three of four");
    // EMERGENCY_CONTROL, FAILURE_RECOVERY, PROGRESS_HEARTBEAT - the declared lane order, not insertion order
    // and not the alphabet.
    let order: Vec<String> = transport
        .sent
        .iter()
        .map(|text| {
            let envelope =
                mayasaba_protocol::envelope::parse_envelope(text).expect("sent envelope");
            format!("{}:{}", envelope.priority(), envelope.message_id())
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "EMERGENCY_CONTROL:msg_control",
            "FAILURE_RECOVERY:msg_recovery",
            "PROGRESS_HEARTBEAT:msg_heartbeat",
        ],
        "bulk traffic must not be served ahead of control or recovery traffic"
    );
    assert_eq!(
        state_of(&bus, "msg_bulk"),
        "QUEUED",
        "the bulk message waits for the next pass"
    );
}

// -----------------------------------------------------------------------------------------------------------
// Backpressure: batch_size bounds one pass, this bounds the queue
// -----------------------------------------------------------------------------------------------------------

#[test]
fn a_pass_reports_how_much_work_is_still_outstanding() {
    let (mut bus, _dir) = {
        let (s, d) = storage_with_project("backlog");
        (Bus::new(s), d)
    };
    let clock = at("2026-10-04T00:00:10Z");
    for n in 1..=3 {
        bus.enqueue(&due_with_identity(
            &format!("msg_b{n}"),
            "prj_backlog",
            n,
            &format!("op_b{n}"),
            "2026-10-04T00:00:00Z",
        ))
        .expect("enqueue");
    }
    assert_eq!(
        bus.backlog().expect("backlog"),
        3,
        "three messages are waiting"
    );

    // Every handover fails, so nothing leaves the queue.
    let mut transport = ScriptedTransport::always(TransportError::Unavailable(
        "the adapter is not running".to_string(),
    ));
    let report = bus.dispatch_due(&clock, &mut transport).expect("pass");
    assert_eq!(report.failed(), 3);
    assert_eq!(
        report.remaining, 3,
        "a pass that handed nothing over has left all of it outstanding"
    );
    assert_eq!(bus.backlog().expect("backlog"), 3);

    // Now every handover succeeds, and the queue empties.
    let mut transport = ScriptedTransport::always_ok();
    let report = bus
        .dispatch_due(&at("2026-10-04T00:10:00Z"), &mut transport)
        .expect("pass");
    assert_eq!(report.dispatched(), 3);
    assert_eq!(
        report.remaining, 0,
        "a message that was handed over is not work still to do"
    );
    assert_eq!(bus.backlog().expect("backlog"), 0);
}

#[test]
fn a_pass_that_empties_the_queue_reports_nothing_outstanding() {
    let (mut bus, _dir) = {
        let (s, d) = storage_with_project("backlog_empty");
        (Bus::new(s), d)
    };
    bus.enqueue(&due_with_identity(
        "msg_be",
        "prj_backlog_empty",
        1,
        "op_be",
        "2026-10-04T00:00:00Z",
    ))
    .expect("enqueue");
    let mut transport = ScriptedTransport::always_ok();
    let report = bus
        .dispatch_due(&at("2026-10-04T00:00:10Z"), &mut transport)
        .expect("pass");
    assert_eq!(report.dispatched(), 1);
    assert_eq!(report.remaining, 0);
}

#[test]
fn a_batch_bound_pass_leaves_the_rest_outstanding() {
    let clock = at("2026-10-04T00:00:10Z");
    // A batch of two, so the pass is bounded and the queue is not emptied - which is exactly the case the report
    // has to make visible, because it is the case where a producer that cannot see the backlog keeps producing.
    let (storage, _dir2) = storage_with_project("backlog_batch");
    let mut bounded = Bus::with_policy(
        storage,
        DispatchPolicy {
            batch_size: 2,
            ..DispatchPolicy::default()
        },
        BackoffPolicy::default(),
    );
    for n in 1..=4 {
        bounded
            .enqueue(&due_with_identity(
                &format!("msg_bb{n}"),
                "prj_backlog_batch",
                n,
                &format!("op_bb{n}"),
                "2026-10-04T00:00:00Z",
            ))
            .expect("enqueue");
    }
    let mut transport = ScriptedTransport::always_ok();
    let report = bounded.dispatch_due(&clock, &mut transport).expect("pass");
    assert_eq!(report.dispatched(), 2);
    assert_eq!(
        report.remaining, 2,
        "the batch bound limited the pass, and what it did not reach is still outstanding"
    );
}
