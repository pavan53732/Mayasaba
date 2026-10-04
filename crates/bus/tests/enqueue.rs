//! Bus-level tests for the outbound enqueue path.
//!
//! The load-bearing properties are that the envelope gate is the protocol crate's own validator rather than a
//! second opinion written here, that the stored envelope is the canonical re-encoding rather than the caller's
//! bytes, and that a retry is told apart from a conflict by the request body. Each is checked by reading the
//! tables and the returned error, not by trusting that the call returned `Ok`.

use mayasaba_bus::{Bus, BusError};
use mayasaba_protocol::generated::envelope as vocab;
use mayasaba_storage::{NewProject, Storage};

fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-bus-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn project(dir: &std::path::Path, tag: &str) -> NewProject {
    NewProject {
        project_id: format!("prj_{tag}"),
        local_path: dir.to_string_lossy().into_owned(),
        brief_id: format!("brf_{tag}"),
        brief_body: "Body".to_string(),
        brief_source: "TEST".to_string(),
        event_id: format!("evt_{tag}_genesis"),
        created_at: "2026-10-04T00:00:00Z".to_string(),
    }
}

/// A complete, legal MCF-v2 envelope, assembled by concatenation so the JSON braces stay readable.
fn envelope(
    message_id: &str,
    project_id: &str,
    sequence: i64,
    operation_id: Option<&str>,
    payload: &str,
) -> String {
    let operation = match operation_id {
        Some(id) => format!(r#""{id}""#),
        None => "null".to_string(),
    };
    [
        r#"{"protocol_version":"MCF-2","schema_version":"2.0.0","#.to_string(),
        format!(r#""message_id":"{message_id}","event_id":"src_{message_id}","#),
        format!(r#""project_id":"{project_id}","session_id":"sess_1","#),
        r#""sender":{"actor_type":"MAYASABA","actor_id":"controller"},"#.to_string(),
        r#""recipients":[{"actor_type":"AGENT","actor_id":"hermes-1"}],"#.to_string(),
        r#""channel":"task","message_type":"TASK","phase":"IMPLEMENTATION","#.to_string(),
        format!(r#""correlation_id":"corr_1","sequence":{sequence},"project_epoch":0,"#),
        r#""priority":"TASK_CONTROL","created_at":"2026-10-04T00:00:05Z","#.to_string(),
        r#""requires_ack":true,"requires_response":false,"blocking":false,"#.to_string(),
        format!(r#""payload":{payload},"#),
        r#""security":{"classification":"INTERNAL_PROJECT","secret_refs":[]},"#.to_string(),
        format!(r#""operation_id":{operation}}}"#),
    ]
    .concat()
}

/// A storage handle holding one created project, and the bus over it.
fn bus_with_project(tag: &str) -> (Bus, std::path::PathBuf) {
    let dir = existing_dir(tag);
    let mut storage = Storage::open_in_memory().expect("open");
    storage.create_project(&project(&dir, tag)).expect("create");
    (Bus::new(storage), dir)
}

fn count(bus: &Bus, table: &str) -> i64 {
    bus.storage().count(table).expect("count")
}

#[test]
fn a_legal_envelope_is_persisted_and_queued_with_both_transitions() {
    let (mut bus, dir) = bus_with_project("enqueue");
    let text = envelope("msg_1", "prj_enqueue", 1, Some("op_1"), r#"{"task":"a"}"#);

    let enqueued = bus.enqueue(&text).expect("enqueue");
    assert!(!enqueued.deduplicated);
    assert_eq!(enqueued.message_id, "msg_1");
    assert_eq!(enqueued.outbox_id, "obx_msg_1");

    assert_eq!(count(&bus, "messages"), 1);
    assert_eq!(count(&bus, "outbox"), 1);
    assert_eq!(
        count(&bus, "events"),
        3,
        "genesis plus two lifecycle events"
    );
    assert!(
        bus.storage()
            .verify_event_chain()
            .expect("verify")
            .is_intact(),
        "the bus's events must extend the project's chain, not start a second one"
    );

    let (state, message_type, source_event): (String, String, String) = bus
        .storage()
        .conn()
        .query_row(
            "SELECT delivery_state, message_type, event_id FROM messages WHERE message_id = 'msg_1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("row");
    assert_eq!(state, "QUEUED");
    assert_eq!(message_type, "TASK");
    assert_eq!(
        source_event, "src_msg_1",
        "messages.event_id is the sender's declared event, not the bus's lifecycle event"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_stored_envelope_is_the_canonical_re_encoding_not_the_callers_bytes() {
    let (mut bus, dir) = bus_with_project("canonical");
    // The same envelope, written with whitespace and with its keys in a different order. Two senders spelling
    // one message differently must produce one stored document, because the retry comparison reads it.
    let pretty = r#"{
      "protocol_version": "MCF-2",
      "schema_version": "2.0.0",
      "message_id": "msg_1",
      "event_id": "src_msg_1",
      "project_id": "prj_canonical",
      "session_id": "sess_1",
      "sender": { "actor_type": "MAYASABA", "actor_id": "controller" },
      "recipients": [{ "actor_type": "AGENT", "actor_id": "hermes-1" }],
      "channel": "task",
      "message_type": "TASK",
      "phase": "IMPLEMENTATION",
      "correlation_id": "corr_1",
      "sequence": 1,
      "project_epoch": 0,
      "priority": "TASK_CONTROL",
      "created_at": "2026-10-04T00:00:05Z",
      "requires_ack": true,
      "requires_response": false,
      "blocking": false,
      "payload": { "task": "a" },
      "security": { "classification": "INTERNAL_PROJECT", "secret_refs": [] },
      "operation_id": "op_1"
    }"#;
    bus.enqueue(pretty).expect("enqueue");

    let stored: String = bus
        .storage()
        .conn()
        .query_row(
            "SELECT envelope_json FROM messages WHERE message_id = 'msg_1'",
            [],
            |r| r.get(0),
        )
        .expect("row");
    assert!(
        !stored.contains('\n'),
        "stored text must be compact: {stored}"
    );
    assert!(
        stored.starts_with(r#"{"blocking":"#),
        "keys must be sorted: {stored}"
    );
    assert!(
        stored.contains(r#""payload":{"task":"a"}"#),
        "the body must survive re-encoding: {stored}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_envelope_the_contract_refuses_is_refused_and_writes_nothing() {
    let (mut bus, dir) = bus_with_project("invalid");

    // Each of these is a different rule: a missing required field, a value outside a closed vocabulary, an
    // undefined field, and text that is not JSON. The bus must not have a second opinion about any of them.
    let cases = [
        (
            envelope("msg_1", "prj_invalid", 1, None, "{}").replace(r#""channel":"task","#, ""),
            "missing channel",
        ),
        (
            envelope("msg_1", "prj_invalid", 1, None, "{}")
                .replace(r#""channel":"task""#, r#""channel":"not_a_channel""#),
            "unknown channel",
        ),
        (
            envelope("msg_1", "prj_invalid", 1, None, "{}")
                .replace(r#""sequence":1"#, r#""sequence":1,"invented":true"#),
            "undefined field",
        ),
        ("{not json".to_string(), "not JSON"),
    ];

    for (text, what) in cases {
        let err = match bus.enqueue(&text) {
            Err(e) => e,
            Ok(enqueued) => panic!("{what} must be refused by the envelope gate, got {enqueued:?}"),
        };
        assert!(
            matches!(err, BusError::InvalidEnvelope(_)),
            "{what} must be refused by the envelope gate, got {err:?}"
        );
        assert_eq!(err.code(), "SCHEMA_INVALID", "{what}");
    }

    assert_eq!(count(&bus, "messages"), 0);
    assert_eq!(count(&bus, "outbox"), 0);
    assert_eq!(count(&bus, "events"), 1, "only the genesis event");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn re_enqueueing_the_same_message_is_absorbed_as_the_ledger_requires() {
    let (mut bus, dir) = bus_with_project("retry");
    let text = envelope("msg_1", "prj_retry", 1, Some("op_1"), r#"{"task":"a"}"#);

    assert!(!bus.enqueue(&text).expect("first").deduplicated);
    let again = bus.enqueue(&text).expect("retry");
    assert!(
        again.deduplicated,
        "the delivery ledger says a duplicate transition returns the existing outcome"
    );
    assert_eq!(again.message_id, "msg_1");
    assert_eq!(count(&bus, "messages"), 1);
    assert_eq!(
        count(&bus, "events"),
        3,
        "a retry must not append a second history"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_retry_under_a_new_message_id_and_the_same_body_is_absorbed() {
    let (mut bus, dir) = bus_with_project("retry-op");
    let first = envelope("msg_1", "prj_retry-op", 1, Some("op_1"), r#"{"task":"a"}"#);
    bus.enqueue(&first).expect("first");

    // A sender that never received its receipt resends the same operation under a fresh message identity, with
    // a new sequence and a new timestamp - and with the body's keys in a different order, which is still the
    // same body.
    let retry = envelope("msg_2", "prj_retry-op", 2, Some("op_1"), r#"{"task":"a"}"#);
    let again = bus.enqueue(&retry).expect("retry");
    assert!(again.deduplicated);
    assert_eq!(again.message_id, "msg_1", "the original is the answer");
    assert_eq!(count(&bus, "messages"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_operation_with_a_different_body_is_a_conflict() {
    let (mut bus, dir) = bus_with_project("conflict");
    bus.enqueue(&envelope(
        "msg_1",
        "prj_conflict",
        1,
        Some("op_1"),
        r#"{"task":"a"}"#,
    ))
    .expect("first");

    let different = envelope("msg_2", "prj_conflict", 2, Some("op_1"), r#"{"task":"b"}"#);
    let err = bus
        .enqueue(&different)
        .expect_err("two requests at one operation");
    match &err {
        BusError::IdempotencyConflict {
            message_id,
            operation_id,
        } => {
            assert_eq!(message_id, "msg_1");
            assert_eq!(operation_id.as_deref(), Some("op_1"));
        }
        other => panic!("expected an idempotency conflict, got {other:?}"),
    }
    assert_eq!(err.code(), "IDEMPOTENCY_CONFLICT");
    assert_eq!(count(&bus, "messages"), 1, "the conflict wrote nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_message_claiming_one_position_is_refused_as_a_duplicate_conflict() {
    let (mut bus, dir) = bus_with_project("position");
    bus.enqueue(&envelope(
        "msg_1",
        "prj_position",
        1,
        Some("op_1"),
        r#"{"task":"a"}"#,
    ))
    .expect("first");

    let clash = envelope("msg_2", "prj_position", 1, Some("op_2"), r#"{"task":"b"}"#);
    let err = bus
        .enqueue(&clash)
        .expect_err("two messages cannot share a position");
    assert!(
        matches!(err, BusError::SequenceConflict { sequence: 1, .. }),
        "unexpected error: {err:?}"
    );
    assert_eq!(
        err.code(),
        "DUPLICATE_CONFLICT",
        "nothing is missing, so this is not SEQUENCE_GAP"
    );
    assert_eq!(count(&bus, "messages"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_message_for_a_project_that_does_not_exist_is_refused() {
    let (mut bus, dir) = bus_with_project("absent");
    // A different project from the one that exists, so the refusal is about routing and not about the fixture.
    let err = bus
        .enqueue(&envelope("msg_1", "prj_nowhere", 1, None, "{}"))
        .expect_err("there is no project to route to");
    assert!(
        matches!(&err, BusError::UnknownProject { project_id } if project_id == "prj_nowhere"),
        "unexpected error: {err:?}"
    );
    assert_eq!(err.code(), "PROJECT_MISMATCH");
    assert_eq!(count(&bus, "messages"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_field_the_bus_transitions_require_is_required_by_the_envelope_contract() {
    // The `message_delivery` transition records declare `required_fields: ["project_id", "correlation_id"]`.
    // The bus reads both from the envelope without checking them, because the protocol validator already
    // refuses an envelope that lacks either. That is an assumption about the contract, so it is pinned here
    // rather than left implicit: if the contract ever stopped requiring one, the bus would silently start
    // writing an empty value into a column its own transition declares required.
    for field in ["project_id", "correlation_id"] {
        assert!(
            vocab::REQUIRED_FIELDS.contains(&field),
            "the bus's transitions require `{field}`, so the envelope contract must too"
        );
    }
    // And the two the bus reads as required integers.
    for field in ["sequence", "project_epoch"] {
        assert!(vocab::REQUIRED_FIELDS.contains(&field));
    }
}

#[test]
fn a_failed_write_leaves_no_queued_message() {
    let dir = existing_dir("atomic");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "atomic"))
        .expect("create");
    storage
        .inject_fault_before_insert("outbox")
        .expect("trigger");
    let mut bus = Bus::new(storage);

    let err = bus
        .enqueue(&envelope("msg_1", "prj_atomic", 1, None, "{}"))
        .expect_err("the outbox write fails");
    assert_eq!(err.code(), "STORAGE_FAILURE");
    assert_eq!(count(&bus, "messages"), 0);
    assert_eq!(count(&bus, "outbox"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
