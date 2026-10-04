//! Startup recovery tests.
//!
//! Atomic creation makes these states impossible through the normal path, so the tests inject them directly.
//! That is the point: recovery exists for state written by something else - an older schema, a build with
//! foreign keys off, or a database that was edited underneath the application. If the atomic path already
//! guaranteed these invariants, recovery would be untestable and therefore unproven.

use mayasaba_storage::{NewProject, Storage};

fn existing_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("mayasaba-rec-{}-{}-{}", std::process::id(), tag, n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn new_project(local_path: &std::path::Path, tag: &str) -> NewProject {
    NewProject {
        project_id: format!("prj_{tag}"),
        local_path: local_path.to_string_lossy().into_owned(),
        brief_id: format!("brf_{tag}"),
        brief_body: format!("Intent for {tag}."),
        brief_source: "TEST".to_string(),
        event_id: format!("evt_{tag}"),
        created_at: "1700000000".to_string(),
    }
}

fn healthy(tag: &str) -> Storage {
    let dir = existing_dir(tag).join("Healthy");
    std::fs::create_dir_all(&dir).unwrap();
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, tag))
        .expect("create");
    storage
}

#[test]
fn a_healthy_database_recovers_clean() {
    let storage = healthy("clean");
    let report = storage.recover().expect("recover");
    assert!(
        report.integrity_ok,
        "SQLite integrity_check must pass on a healthy database"
    );
    assert!(
        report.is_clean(),
        "a correctly created project must produce no recovery issues: {:?}",
        report.issues
    );
    assert!(report.issues.is_empty());
}

#[test]
fn an_empty_database_recovers_clean() {
    let storage = Storage::open_in_memory().expect("open");
    let report = storage.recover().expect("recover");
    assert!(
        report.is_clean(),
        "a fresh install has nothing to recover from: {:?}",
        report.issues
    );
}

#[test]
fn a_project_without_its_brief_is_reported() {
    // The intent-anchor invariant, broken directly. Atomic creation cannot produce this, so if it exists the
    // database was written by something else and the Control Room must be told rather than showing a project
    // whose intent is unknown.
    let dir = existing_dir("nobrief").join("NoBrief");
    std::fs::create_dir_all(&dir).unwrap();
    let storage = Storage::open_in_memory().expect("open");

    storage
        .conn()
        .execute(
            "INSERT INTO projects (project_id, name, local_path, phase, status, current_epoch, created_at, updated_at)
             VALUES ('prj_orphan', 'NoBrief', ?1, 'DISCOVERY', 'ACTIVE', 0, '1700000000', '1700000000')",
            [dir.to_string_lossy()],
        )
        .expect("inject project without a brief");

    let report = storage.recover().expect("recover");
    assert!(!report.is_clean());
    let issue = report
        .issues
        .iter()
        .find(|i| i.kind == "PROJECT_WITHOUT_BRIEF")
        .expect("missing-brief issue");
    assert!(
        issue.detail.contains("prj_orphan"),
        "the issue must name the project: {}",
        issue.detail
    );
}

#[test]
fn epoch_summary_drift_is_reported() {
    // The summary column and the epoch history disagree, so a context digest computed from one would be
    // wrong for the other. This is exactly the kind of silent inconsistency that is worse than a crash.
    let storage = healthy("drift");
    storage
        .conn()
        .execute(
            "UPDATE projects SET current_epoch = 7 WHERE project_id = 'prj_drift'",
            [],
        )
        .expect("inject drift");

    let report = storage.recover().expect("recover");
    let issue = report
        .issues
        .iter()
        .find(|i| i.kind == "EPOCH_SUMMARY_DRIFT")
        .expect("drift issue");
    assert!(
        issue.detail.contains("7"),
        "the issue must state both values: {}",
        issue.detail
    );
}

#[test]
fn orphaned_rows_are_reported() {
    // Foreign keys are per-connection, so rows written by a build that had them off survive a reopen with
    // them on. This test reproduces exactly that: it turns FKs off, writes the bad row, and then recovery -
    // running with FKs enforced, as the real application does - must still notice it.
    let storage = Storage::open_in_memory().expect("open");
    storage
        .conn()
        .execute_batch("PRAGMA foreign_keys = OFF")
        .expect("simulate a build with FKs off");
    storage
        .conn()
        .execute(
            "INSERT INTO project_briefs (brief_id, project_id, version, body, source, created_at)
             VALUES ('brf_ghost', 'prj_missing', 1, 'body', 'TEST', '1700000000')",
            [],
        )
        .expect("inject orphan brief");
    storage
        .conn()
        .execute_batch("PRAGMA foreign_keys = ON")
        .expect("restore FK enforcement");

    let report = storage.recover().expect("recover");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.kind == "ORPHANED_PROJECT_ROWS"),
        "an orphaned brief must be reported even with foreign keys now enforced: {:?}",
        report.issues
    );
}

#[test]
fn recovery_reports_and_never_repairs() {
    // A silent repair would let a reader believe the database says something it does not. Recovery is
    // read-only: the bad state must still be there afterwards, for an owning service to decide about.
    let storage = Storage::open_in_memory().expect("open");
    storage
        .conn()
        .execute(
            "INSERT INTO projects (project_id, name, local_path, phase, status, current_epoch, created_at, updated_at)
             VALUES ('prj_bad', 'Bad', 'C:\\temp', 'DISCOVERY', 'ACTIVE', 0, '1', '1')",
            [],
        )
        .expect("inject project without a brief");

    let _ = storage.recover().expect("recover");
    let still_there: i64 = storage
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE project_id = 'prj_bad'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(
        still_there, 1,
        "recovery must report, not silently delete authoritative rows"
    );
}

#[test]
fn several_problems_are_all_reported_not_just_the_first() {
    // A scan that stops at the first problem would force an operator to restart repeatedly to discover the
    // rest, and would hide the scale of the damage.
    let storage = Storage::open_in_memory().expect("open");
    for id in ["prj_a", "prj_b", "prj_c"] {
        storage
            .conn()
            .execute(
                "INSERT INTO projects (project_id, name, local_path, phase, status, current_epoch, created_at, updated_at)
                 VALUES (?1, ?1, 'C:\\temp', 'DISCOVERY', 'ACTIVE', 0, '1', '1')",
                [id],
            )
            .expect("inject project");
    }

    let report = storage.recover().expect("recover");
    let missing = report
        .issues
        .iter()
        .filter(|i| i.kind == "PROJECT_WITHOUT_BRIEF")
        .count();
    assert_eq!(
        missing, 3,
        "every affected project must appear, not just the first"
    );
}
