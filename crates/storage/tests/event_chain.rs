//! Storage-level tests for the DEC-034 event hash chain.
//!
//! `crates/storage/src/chain.rs` tests the chain arithmetic against synthetic rows. These tests exercise the
//! same rule through the real database: a project's genesis link written by `create_project`, later links
//! written by `append_event`, and the detection of an edit, a deletion and an unpositioned row performed
//! directly on the table with SQL, which is exactly the threat the chain exists for.
//!
//! The load-bearing assertion is not "verification returned an error". It is that the stored `event_hash`
//! equals the hash recomputed from the stored row by the declared rule, so the chain cannot be self-consistent
//! and wrong.

use mayasaba_storage::chain::{
    event_hash, ChainScope, DivergenceKind, EventHashFields, GENESIS_PREV_HASH,
};
use mayasaba_storage::{NewEvent, NewProject, Storage};

fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-chain-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn new_project(dir: &std::path::Path, tag: &str) -> NewProject {
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

fn event(project: Option<&str>, session: Option<&str>, sequence: i64, id: &str) -> NewEvent {
    NewEvent {
        event_id: id.to_string(),
        project_id: project.map(str::to_string),
        session_id: session.map(str::to_string),
        event_type: "TASK_PROGRESS".to_string(),
        sequence,
        correlation_id: Some("corr_1".to_string()),
        causation_id: None,
        epoch: Some(0),
        payload_json: format!(r#"{{"event":"{id}"}}"#),
        created_at: "2026-10-04T00:00:01Z".to_string(),
    }
}

/// The stored row for `event_id`, hashed by the declared rule and compared with what was persisted.
fn assert_stored_link_is_recomputable(storage: &Storage, event_id: &str) {
    let row = storage
        .conn()
        .query_row(
            "SELECT project_id, session_id, event_type, sequence, correlation_id, causation_id, epoch,
                    payload_json, created_at, prev_hash, event_hash
             FROM events WHERE event_id = ?1",
            rusqlite::params![event_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, String>(10)?,
                ))
            },
        )
        .expect("event row");

    let recomputed = event_hash(&EventHashFields {
        prev_hash: &row.9,
        event_id,
        project_id: row.0.as_deref(),
        session_id: row.1.as_deref(),
        event_type: &row.2,
        sequence: row.3,
        correlation_id: row.4.as_deref(),
        causation_id: row.5.as_deref(),
        epoch: row.6,
        payload_json: &row.7,
        created_at: &row.8,
    })
    .expect("canonical");

    assert_eq!(
        recomputed, row.10,
        "the stored event_hash for {event_id} must be the declared rule's output, not a value written alongside it"
    );
}

#[test]
fn project_creation_writes_a_genesis_link_that_verifies() {
    let dir = existing_dir("genesis");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "genesis"))
        .expect("create");

    let report = storage.verify_event_chain().expect("verify");
    assert!(report.is_intact(), "unexpected divergences: {report:?}");
    assert_eq!(report.chains, 1);
    assert_eq!(report.events, 1);

    let prev: String = storage
        .conn()
        .query_row(
            "SELECT prev_hash FROM events WHERE event_id = 'evt_genesis_genesis'",
            [],
            |r| r.get(0),
        )
        .expect("genesis row");
    assert_eq!(
        prev, GENESIS_PREV_HASH,
        "the first event of a project chain must name the 64-zero genesis hash"
    );
    assert_eq!(prev.len(), 64);

    assert_stored_link_is_recomputable(&storage, "evt_genesis_genesis");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn appended_events_extend_the_same_chain() {
    let dir = existing_dir("append");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "append"))
        .expect("create");
    storage
        .append_event(&event(Some("prj_append"), None, 2, "evt_append_2"))
        .expect("append");
    storage
        .append_event(&event(Some("prj_append"), None, 3, "evt_append_3"))
        .expect("append");

    let report = storage.verify_event_chain().expect("verify");
    assert!(report.is_intact(), "unexpected divergences: {report:?}");
    assert_eq!(report.chains, 1);
    assert_eq!(report.events, 3);

    // Each link names the previous link's hash, so the chain is a chain and not three independent hashes.
    let genesis: String = storage
        .conn()
        .query_row(
            "SELECT event_hash FROM events WHERE event_id = 'evt_append_genesis'",
            [],
            |r| r.get(0),
        )
        .expect("genesis");
    let second_prev: String = storage
        .conn()
        .query_row(
            "SELECT prev_hash FROM events WHERE event_id = 'evt_append_2'",
            [],
            |r| r.get(0),
        )
        .expect("second");
    assert_eq!(second_prev, genesis);
    assert_ne!(second_prev, GENESIS_PREV_HASH);

    assert_stored_link_is_recomputable(&storage, "evt_append_2");
    assert_stored_link_is_recomputable(&storage, "evt_append_3");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tampered_payload_is_detected_and_recovery_reports_it() {
    let dir = existing_dir("tamper");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "tamper"))
        .expect("create");
    storage
        .append_event(&event(Some("prj_tamper"), None, 2, "evt_tamper_2"))
        .expect("append");
    assert!(storage.verify_event_chain().expect("verify").is_intact());

    // The threat the chain exists for: a row edited after the fact, without going through the chain writer.
    storage
        .conn()
        .execute(
            "UPDATE events SET payload_json = '{\"tampered\":true}' WHERE event_id = 'evt_tamper_genesis'",
            [],
        )
        .expect("tamper");

    let report = storage.verify_event_chain().expect("verify");
    assert_eq!(
        report.first_divergence().expect("first").event_id,
        "evt_tamper_genesis"
    );
    assert_eq!(
        report.first_divergence().expect("first").kind,
        DivergenceKind::EventHashMismatch
    );
    // The later link must break too, because it names the recomputed hash of its predecessor.
    assert_eq!(
        report.divergences.last().expect("last").event_id,
        "evt_tamper_2"
    );
    assert_eq!(
        report.divergences.last().expect("last").kind,
        DivergenceKind::PrevHashMismatch
    );

    // A startup recovery scan must surface it, or a tampered database would report itself clean.
    let recovery = storage.recover().expect("recover");
    assert!(!recovery.is_clean());
    assert!(
        recovery
            .issues
            .iter()
            .any(|i| i.kind == "EVENT_CHAIN_BROKEN"),
        "recovery must report a broken chain: {:?}",
        recovery.issues
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_event_is_detected_at_its_successor() {
    let dir = existing_dir("delete");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "delete"))
        .expect("create");
    storage
        .append_event(&event(Some("prj_delete"), None, 2, "evt_delete_2"))
        .expect("append");
    storage
        .append_event(&event(Some("prj_delete"), None, 3, "evt_delete_3"))
        .expect("append");

    storage
        .conn()
        .execute("DELETE FROM events WHERE event_id = 'evt_delete_2'", [])
        .expect("delete");

    let report = storage.verify_event_chain().expect("verify");
    assert_eq!(report.events, 2);
    assert_eq!(
        report.first_divergence().expect("first").event_id,
        "evt_delete_3"
    );
    assert_eq!(
        report.first_divergence().expect("first").kind,
        DivergenceKind::PrevHashMismatch
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_projects_chain_independently_through_storage() {
    let first = existing_dir("two-a");
    let second = existing_dir("two-b");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&first, "twoa"))
        .expect("create");
    storage
        .create_project(&new_project(&second, "twob"))
        .expect("create");
    storage
        .append_event(&event(Some("prj_twoa"), None, 2, "evt_twoa_2"))
        .expect("append");
    storage
        .append_event(&event(Some("prj_twob"), None, 2, "evt_twob_2"))
        .expect("append");

    let report = storage.verify_event_chain().expect("verify");
    assert!(report.is_intact(), "unexpected divergences: {report:?}");
    assert_eq!(report.chains, 2);
    assert_eq!(report.events, 4);

    // Both second links name their own project's genesis hash, not the other project's.
    let a_genesis: String = storage
        .conn()
        .query_row(
            "SELECT event_hash FROM events WHERE event_id = 'evt_twoa_genesis'",
            [],
            |r| r.get(0),
        )
        .expect("a genesis");
    let b_prev: String = storage
        .conn()
        .query_row(
            "SELECT prev_hash FROM events WHERE event_id = 'evt_twob_2'",
            [],
            |r| r.get(0),
        )
        .expect("b prev");
    assert_ne!(a_genesis, b_prev, "the two chains must not be linked");
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

#[test]
fn a_session_scoped_chain_is_separate_from_a_project_chain() {
    let dir = existing_dir("session");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "session"))
        .expect("create");
    storage
        .append_event(&event(None, Some("sess_1"), 1, "evt_sess_1"))
        .expect("append");
    storage
        .append_event(&event(None, Some("sess_1"), 2, "evt_sess_2"))
        .expect("append");

    let report = storage.verify_event_chain().expect("verify");
    assert!(report.is_intact(), "unexpected divergences: {report:?}");
    assert_eq!(
        report.chains, 2,
        "the project chain and the session chain are distinct"
    );
    assert_eq!(report.events, 3);

    let session_genesis: String = storage
        .conn()
        .query_row(
            "SELECT prev_hash FROM events WHERE event_id = 'evt_sess_1'",
            [],
            |r| r.get(0),
        )
        .expect("session genesis");
    assert_eq!(session_genesis, GENESIS_PREV_HASH);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_event_naming_neither_a_project_nor_a_session_is_refused_and_persists_nothing() {
    let mut storage = Storage::open_in_memory().expect("open");
    let err = storage
        .append_event(&event(None, None, 1, "evt_orphan"))
        .expect_err("an unscoped event belongs to no chain");
    assert!(
        matches!(err, mayasaba_storage::StorageError::UnscopedEvent),
        "unexpected error: {err:?}"
    );
    assert_eq!(storage.count("events").expect("count"), 0);
}

#[test]
fn an_unpositioned_row_is_reported_rather_than_silently_chained() {
    let dir = existing_dir("unpositioned");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "unpositioned"))
        .expect("create");

    // `events.sequence` is nullable and this crate never writes a null, so the only way to produce one is
    // outside the chain writer - which is the case being pinned.
    storage
        .conn()
        .execute(
            "INSERT INTO events (event_id, project_id, event_type, sequence, payload_json, prev_hash, event_hash, created_at)
             VALUES ('evt_manual', 'prj_unpositioned', 'TASK_PROGRESS', NULL, '{}', 'x', 'y', '2026-10-04T00:00:02Z')",
            [],
        )
        .expect("insert unpositioned row");

    let report = storage.verify_event_chain().expect("verify");
    assert_eq!(report.events, 1, "the unpositioned row is not walked");
    assert!(
        report
            .divergences
            .iter()
            .any(|d| d.kind == DivergenceKind::Unsequenced && d.event_id == "evt_manual"),
        "an event with no declared position must be reported: {report:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn appending_an_event_rolls_back_completely_when_the_insert_fails() {
    let dir = existing_dir("atomic-append");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "atomic-append"))
        .expect("create");
    storage
        .inject_fault_before_insert("events")
        .expect("trigger");

    assert!(storage
        .append_event(&event(Some("prj_atomic-append"), None, 2, "evt_atomic"))
        .is_err());
    assert_eq!(
        storage.count("events").expect("count"),
        1,
        "only the genesis event may exist after a failed append"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_chain_scope_of_an_event_is_derived_from_its_own_identifiers() {
    // A project and a session that share a spelling must not be merged into one chain. The scope type keeps
    // the kind apart, and this pins that through the storage path rather than through the pure function only.
    let dir = existing_dir("shared-spelling");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "shared-spelling"))
        .expect("create");
    storage
        .append_event(&event(
            None,
            Some("prj_shared-spelling"),
            1,
            "evt_shared_session",
        ))
        .expect("append");

    let report = storage.verify_event_chain().expect("verify");
    assert!(report.is_intact(), "unexpected divergences: {report:?}");
    assert_eq!(report.chains, 2);
    assert_eq!(
        ChainScope::of(Some("x"), None).expect("scoped").column(),
        "project_id"
    );
    assert_eq!(
        ChainScope::of(None, Some("x")).expect("scoped").column(),
        "session_id"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
