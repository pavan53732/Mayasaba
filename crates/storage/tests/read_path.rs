//! Read-path tests: a persisted project must be rehydratable exactly as it was written.
//!
//! This is the second half of the persistence boundary. `CREATE` persisting a project proves the write; these
//! prove that what comes back out is the same authoritative state, which is what makes the Control Room a
//! rehydrating application rather than an intake wizard that forgets.

use mayasaba_storage::{NewProject, Storage};

fn existing_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mayasaba-read-{}-{}-{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn new_project(local_path: &std::path::Path, tag: &str, created_at: &str) -> NewProject {
    NewProject {
        project_id: format!("prj_{tag}"),
        local_path: local_path.to_string_lossy().into_owned(),
        brief_id: format!("brf_{tag}"),
        brief_body: format!("Intent for {tag}."),
        brief_source: "TEST".to_string(),
        event_id: format!("evt_{tag}"),
        created_at: created_at.to_string(),
    }
}

#[test]
fn a_created_project_is_read_back_identically() {
    let dir = existing_dir("roundtrip").join("RoundTrip");
    std::fs::create_dir_all(&dir).unwrap();

    let mut storage = Storage::open_in_memory().expect("open");
    storage.create_project(&new_project(&dir, "roundtrip", "1700000000")).expect("create");

    let record = storage.get_project("prj_roundtrip").expect("read back");
    assert_eq!(record.project_id, "prj_roundtrip");
    assert_eq!(record.name, "RoundTrip", "the name survives as the derived folder leaf");
    assert_eq!(record.local_path, dir.to_string_lossy());
    assert_eq!(record.phase, "DISCOVERY");
    assert_eq!(record.status, "ACTIVE");
    assert_eq!(record.current_epoch, 0);
    assert_eq!(record.brief_id.as_deref(), Some("brf_roundtrip"));
    assert_eq!(record.brief_version, Some(1));
    assert_eq!(record.brief_body.as_deref(), Some("Intent for roundtrip."));
    assert_eq!(record.created_at, "1700000000", "the read path returns creation time, needed to order the list");
}

#[test]
fn listing_returns_every_project_newest_first() {
    let mut storage = Storage::open_in_memory().expect("open");

    for (tag, leaf, at) in [("old", "Oldest", "1700000001"), ("mid", "Middle", "1700000002"), ("new", "Newest", "1700000003")] {
        let dir = existing_dir(tag).join(leaf);
        std::fs::create_dir_all(&dir).unwrap();
        storage.create_project(&new_project(&dir, tag, at)).expect("create");
    }

    let listed = storage.list_projects().expect("list");
    assert_eq!(listed.len(), 3);
    assert_eq!(
        listed.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["Newest", "Middle", "Oldest"],
        "the Control Room list must not depend on insertion order or a planner's whim"
    );
}

#[test]
fn listing_an_empty_database_is_empty_not_an_error() {
    let storage = Storage::open_in_memory().expect("open");
    assert!(storage.list_projects().expect("list").is_empty(), "a fresh install has no projects, which is not a failure");
}

#[test]
fn a_persisted_project_survives_reopening_the_file() {
    // The durable claim: not an in-memory artefact. This is the storage-level half of the restart proof; the
    // desktop process half still needs a real application restart.
    let dir = std::env::temp_dir().join(format!("mayasaba-read-db-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ws = existing_dir("durable").join("Durable");
    std::fs::create_dir_all(&ws).unwrap();

    {
        let mut storage = Storage::open(&dir.join("mayasaba.sqlite3")).expect("open");
        storage.create_project(&new_project(&ws, "durable", "1700000009")).expect("create");
    }

    let reopened = Storage::open(&dir.join("mayasaba.sqlite3")).expect("reopen");
    let listed = reopened.list_projects().expect("list after reopen");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Durable");
    assert_eq!(listed[0].brief_body.as_deref(), Some("Intent for durable."));
    assert_eq!(listed[0].current_epoch, 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reading_an_unknown_project_is_not_found_rather_than_an_empty_record() {
    let storage = Storage::open_in_memory().expect("open");
    let err = storage.get_project("prj_nope").expect_err("must not resolve");
    assert!(
        matches!(err, mayasaba_storage::StorageError::NotFound(_)),
        "an unknown project must be NotFound, or the Control Room could render a project that does not exist"
    );
}