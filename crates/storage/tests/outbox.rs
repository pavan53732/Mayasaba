//! Storage-level tests for the durable outbox write.
//!
//! The load-bearing properties are that the four writes of an enqueue are one transaction, that the message's
//! lifecycle is recorded as the two transitions the `message_delivery` machine declares rather than one, and
//! that a duplicate is absorbed by identity rather than by luck. Each is checked by reading the tables, not by
//! trusting the return value.

use mayasaba_storage::{NewOutboundMessage, NewProject, Storage, StorageError};

fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-outbox-{}-{}", std::process::id(), tag));
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

/// An envelope carrying `operation_id`, so the idempotency lookup has something to extract.
fn envelope(
    project_id: &str,
    message_id: &str,
    sequence: i64,
    operation_id: Option<&str>,
) -> String {
    let operation = match operation_id {
        Some(id) => format!(r#""{id}""#),
        None => "null".to_string(),
    };
    format!(
        r#"{{"protocol_version":"MCF-2","message_id":"{message_id}","project_id":"{project_id}","sequence":{sequence},"operation_id":{operation}}}"#
    )
}

fn message(project_id: &str, tag: &str, sequence: i64) -> NewOutboundMessage {
    let message_id = format!("msg_{tag}");
    NewOutboundMessage {
        event_id: format!("src_{tag}"),
        project_id: project_id.to_string(),
        session_id: "sess_1".to_string(),
        message_type: "TASK".to_string(),
        channel: "task".to_string(),
        sequence,
        correlation_id: "corr_1".to_string(),
        causation_id: None,
        idempotency_key: Some(format!("idem_{tag}")),
        operation_id: Some(format!("op_{tag}")),
        envelope_json: envelope(
            project_id,
            &message_id,
            sequence,
            Some(&format!("op_{tag}")),
        ),
        created_at: "2026-10-04T00:00:05Z".to_string(),
        project_epoch: 0,
        message_id,
    }
}

fn count(storage: &Storage, table: &str) -> i64 {
    storage.count(table).expect("count")
}

#[test]
fn an_enqueue_writes_the_message_the_queue_entry_and_both_transitions() {
    let dir = existing_dir("basic");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "basic"))
        .expect("create");

    let enqueued = storage
        .enqueue_message(&message("prj_basic", "a", 1))
        .expect("enqueue");
    assert!(!enqueued.deduplicated);
    assert_eq!(enqueued.message_id, "msg_a");
    assert_eq!(enqueued.outbox_id, "obx_msg_a");

    assert_eq!(count(&storage, "messages"), 1);
    assert_eq!(count(&storage, "outbox"), 1);
    // Genesis, MESSAGE_PERSISTED, MESSAGE_QUEUED. Two lifecycle events, because the machine declares two
    // transitions and each has its own canonical event.
    assert_eq!(count(&storage, "events"), 3);

    let (state, outbox_state, attempts): (String, String, i64) = storage
        .conn()
        .query_row(
            "SELECT m.delivery_state, o.dispatch_state, o.attempts
             FROM messages m JOIN outbox o ON o.message_id = m.message_id
             WHERE m.message_id = 'msg_a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("row");
    assert_eq!(
        state, "QUEUED",
        "an enqueued message is queued, not merely persisted"
    );
    assert_eq!(outbox_state, "PENDING");
    assert_eq!(attempts, 0);

    // The events are the machine's own two steps, in order, and each carries the sender's correlation.
    // `correlation_id` is nullable on `events` and null on a project's genesis event, so it is read as an
    // `Option` rather than as a string that would panic on the one row that has none.
    let events: Vec<(String, i64, Option<String>)> = {
        let mut stmt = storage
            .conn()
            .prepare(
                "SELECT event_type, sequence, correlation_id FROM events
                 WHERE project_id = 'prj_basic' ORDER BY sequence",
            )
            .expect("prepare");
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("query");
        rows.map(|r| r.expect("row")).collect()
    };
    assert_eq!(
        events
            .iter()
            .map(|(kind, _, _)| kind.as_str())
            .collect::<Vec<_>>(),
        vec!["PROJECT_CREATED", "MESSAGE_PERSISTED", "MESSAGE_QUEUED"]
    );
    assert_eq!(
        events.iter().map(|(_, seq, _)| *seq).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "the derived positions continue the project's chain rather than restarting it"
    );
    assert_eq!(events[0].2, None, "the genesis event has no correlation");
    assert!(events[1..]
        .iter()
        .all(|(_, _, corr)| corr.as_deref() == Some("corr_1")));

    let report = storage.verify_event_chain().expect("verify");
    assert!(report.is_intact(), "unexpected divergences: {report:?}");
    assert_eq!(report.chains, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_transition_payload_is_json_even_when_the_values_need_escaping() {
    // `message_id` is only `format`-annotated in the contract, and JSON Schema treats `format` as annotation,
    // so the validator admits a message id containing a quote. Interpolating that into the payload would store
    // a document no parser accepts - and DEC-034 hashes that text.
    let dir = existing_dir("escaping");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "escaping"))
        .expect("create");

    let mut hostile = message("prj_escaping", "hostile", 1);
    hostile.message_id = "msg_\"quoted\"\\back".to_string();
    // A valid envelope whose own message id is escaped the way JSON requires, so this test is about the payload
    // the crate builds and not about the text it is handed.
    hostile.envelope_json =
        r#"{"message_id":"msg_\"quoted\"\\back","operation_id":"op_hostile"}"#.to_string();
    storage.enqueue_message(&hostile).expect("enqueue");

    let payload: String = storage
        .conn()
        .query_row(
            "SELECT payload_json FROM events WHERE event_type = 'MESSAGE_PERSISTED'",
            [],
            |r| r.get(0),
        )
        .expect("payload");
    assert_eq!(
        payload,
        r#"{"channel":"task","message_id":"msg_\"quoted\"\\back","message_type":"TASK","sequence":1}"#,
        "the payload must be a canonical document, not interpolated text"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_genesis_payload_is_json_even_though_a_windows_path_contains_backslashes() {
    // The defect this pins: `local_path` was interpolated into the genesis payload, and a Windows path is full
    // of backslashes, so every stored genesis payload was invalid JSON.
    let dir = existing_dir("genesis-json");
    let mut storage = Storage::open_in_memory().expect("open");
    let new = project(&dir, "genesis-json");
    storage.create_project(&new).expect("create");

    let payload: String = storage
        .conn()
        .query_row(
            "SELECT payload_json FROM events WHERE event_type = 'PROJECT_CREATED'",
            [],
            |r| r.get(0),
        )
        .expect("payload");
    let escaped_path = new.local_path.replace('\\', "\\\\");
    assert_eq!(
        payload,
        format!(r#"{{"brief_id":"brf_genesis-json","epoch":0,"local_path":"{escaped_path}"}}"#),
        "a backslash in a Windows path must be escaped, not emitted raw"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn re_enqueueing_the_same_message_is_absorbed_by_identity() {
    let dir = existing_dir("dedupe-message");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "dedupe-message"))
        .expect("create");

    let first = storage
        .enqueue_message(&message("prj_dedupe-message", "a", 1))
        .expect("enqueue");
    let again = storage
        .enqueue_message(&message("prj_dedupe-message", "a", 1))
        .expect("re-enqueue");

    assert!(!first.deduplicated);
    assert!(
        again.deduplicated,
        "the same message id is the same message"
    );
    assert_eq!(again.message_id, first.message_id);
    assert_eq!(again.outbox_id, first.outbox_id);
    assert!(
        again.stored_envelope_json.is_some(),
        "the caller needs the stored text to tell a retry from a conflict"
    );
    assert_eq!(count(&storage, "messages"), 1);
    assert_eq!(count(&storage, "outbox"), 1);
    assert_eq!(count(&storage, "events"), 3, "no second pair of events");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_retry_of_the_same_operation_under_a_new_message_id_is_absorbed() {
    let dir = existing_dir("dedupe-operation");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "dedupe-operation"))
        .expect("create");

    let first = storage
        .enqueue_message(&message("prj_dedupe-operation", "a", 1))
        .expect("enqueue");

    // Same operation, new message identity - which is what a sender that did not receive its receipt does.
    let mut retry = message("prj_dedupe-operation", "b", 2);
    retry.operation_id = Some("op_a".to_string());
    retry.envelope_json = envelope("prj_dedupe-operation", &retry.message_id, 2, Some("op_a"));
    let again = storage.enqueue_message(&retry).expect("retry");

    assert!(
        again.deduplicated,
        "project_id + operation_id is the idempotency scope"
    );
    assert_eq!(
        again.message_id, first.message_id,
        "the original is the answer"
    );
    assert_eq!(count(&storage, "messages"), 1);

    let found = storage
        .find_outbound_by_operation("prj_dedupe-operation", "op_a")
        .expect("lookup")
        .expect("present");
    assert_eq!(found.message_id, first.message_id);
    assert!(storage
        .find_outbound_by_operation("prj_dedupe-operation", "op_nonexistent")
        .expect("lookup")
        .is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_position_claimed_by_a_different_message_is_refused_by_name() {
    let dir = existing_dir("position");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "position"))
        .expect("create");
    storage
        .enqueue_message(&message("prj_position", "a", 1))
        .expect("enqueue");

    // Different message, different operation, same (session, channel, sequence).
    let mut clash = message("prj_position", "b", 1);
    clash.operation_id = Some("op_b".to_string());
    clash.envelope_json = envelope("prj_position", "msg_b", 1, Some("op_b"));
    let err = storage
        .enqueue_message(&clash)
        .expect_err("two messages cannot occupy one position");

    match err {
        StorageError::SequenceConflict {
            session_id,
            channel,
            sequence,
        } => {
            assert_eq!(session_id, "sess_1");
            assert_eq!(channel, "task");
            assert_eq!(sequence, 1);
        }
        other => panic!("expected a named sequence conflict, got {other:?}"),
    }
    assert_eq!(count(&storage, "messages"), 1, "the refusal wrote nothing");
    assert_eq!(count(&storage, "events"), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unknown_project_is_refused_by_name() {
    let mut storage = Storage::open_in_memory().expect("open");
    let err = storage
        .enqueue_message(&message("prj_absent", "a", 1))
        .expect_err("a message for no project cannot be queued");
    match err {
        StorageError::UnknownProject { project_id } => assert_eq!(project_id, "prj_absent"),
        other => panic!("expected a named unknown project, got {other:?}"),
    }
    assert_eq!(count(&storage, "messages"), 0);
    assert_eq!(count(&storage, "outbox"), 0);
    assert_eq!(count(&storage, "events"), 0);
}

#[test]
fn an_enqueue_is_one_transaction_so_a_failure_leaves_no_trace() {
    let dir = existing_dir("atomic");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "atomic"))
        .expect("create");

    // The outbox insert is the third of the four writes, so a failure there must undo the message row and the
    // MESSAGE_PERSISTED event that preceded it.
    storage
        .inject_fault_before_insert("outbox")
        .expect("trigger");
    assert!(storage
        .enqueue_message(&message("prj_atomic", "a", 1))
        .is_err());

    assert_eq!(count(&storage, "messages"), 0, "no half-queued message");
    assert_eq!(count(&storage, "outbox"), 0);
    assert_eq!(
        count(&storage, "events"),
        1,
        "only the genesis event survives"
    );
    assert!(storage.verify_event_chain().expect("verify").is_intact());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn text_that_is_not_json_is_refused_the_same_way_whether_or_not_it_names_an_operation() {
    // Without an explicit check this input behaves differently depending on an unrelated field: the idempotency
    // lookup calls `json_extract`, which raises on malformed input, so a message carrying an `operation_id`
    // would fail and the identical message without one would be stored.
    let dir = existing_dir("malformed-json");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "malformed-json"))
        .expect("create");

    for operation in [Some("op_x"), None] {
        let mut broken = message("prj_malformed-json", "a", 1);
        broken.operation_id = operation.map(str::to_string);
        broken.envelope_json = "{not json".to_string();
        match storage.enqueue_message(&broken) {
            Err(StorageError::MalformedJson { column }) => {
                assert_eq!(column, "messages.envelope_json")
            }
            other => panic!("expected a named malformed-JSON refusal, got {other:?}"),
        }
    }
    assert_eq!(count(&storage, "messages"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_operation_lookup_is_indexed_rather_than_a_scan() {
    // The lookup reads `operation_id` out of `envelope_json`. This pins that the expression index the schema
    // declares was actually created, because an index that silently failed to apply would leave the query
    // correct and slow, and nothing else in the suite would notice.
    let storage = Storage::open_in_memory().expect("open");
    let plan: String = storage
        .conn()
        .query_row(
            "SELECT group_concat(name) FROM sqlite_master WHERE type = 'index' AND name = 'idx_messages_operation'",
            [],
            |r| r.get(0),
        )
        .expect("index row");
    assert_eq!(plan, "idx_messages_operation");
}

#[test]
fn two_sessions_on_one_channel_do_not_share_a_position_namespace() {
    let dir = existing_dir("two-sessions");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&project(&dir, "two-sessions"))
        .expect("create");

    let mut first = message("prj_two-sessions", "a", 1);
    first.session_id = "sess_1".to_string();
    let mut second = message("prj_two-sessions", "b", 1);
    second.session_id = "sess_2".to_string();

    storage.enqueue_message(&first).expect("first");
    storage.enqueue_message(&second).expect(
        "the uniqueness is per session and channel, so sequence 1 is free in another session",
    );
    assert_eq!(count(&storage, "messages"), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
