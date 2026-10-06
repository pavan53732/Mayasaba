//! UserContribution slice tests: recording a post-creation free-text contribution end to end.
//!
//! These exercise the properties DEC-030 makes load-bearing rather than the shape of the API:
//!
//! - a contribution is recorded with the advisory classification it was routed under, and that label is
//!   advisory: recording one labelled `MATERIAL` does not itself advance the project epoch, because only the
//!   owning authoritative service determines materiality;
//! - the epoch pair recorded is the project's own, read by the service, so a caller cannot record an effect it
//!   chose;
//! - an unknown project, an undeclared classification and a whitespace-only body are each refused before
//!   anything is written, so a refusal leaves no row behind;
//! - the stored body is the trimmed text that was validated.
//!
//! Rows are read back through `mayasaba-storage` directly rather than through the service, so these assertions
//! are about what is durable rather than about what the service returned.

use std::path::{Path, PathBuf};

use mayasaba_core::project_service::{
    CreateProjectError, CreateProjectOutcome, CreateProjectRequest, ProjectService,
    ProjectValidationError, RecordContributionRequest,
};
use mayasaba_storage::Storage;

/// A unique database file per call, so parallel tests in one process cannot share durable state.
fn temp_db(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let db = std::env::temp_dir().join(format!(
        "mayasaba-contrib-db-{}-{}-{}.sqlite",
        std::process::id(),
        tag,
        n
    ));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{}", db.to_string_lossy(), suffix));
    }
    db
}

/// A real workspace folder, unique per call.
fn temp_workspace(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mayasaba-contrib-ws-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workspace dir");
    dir
}

/// Create a real project through the owning service, so a contribution has a genuine project row to belong to.
fn create_project(db: &Path, workspace: &Path) -> String {
    let mut service = ProjectService::open(db).expect("project service");
    let outcome = service
        .create_project(&CreateProjectRequest {
            local_path: workspace.to_string_lossy().into_owned(),
            initial_brief_body: "UserContribution slice test project.".to_string(),
            brief_source: None,
        })
        .expect("create project");
    match outcome {
        CreateProjectOutcome::Created { project } => project.project_id,
    }
}

fn record(project_id: &str, body: &str, classification: &str) -> RecordContributionRequest {
    RecordContributionRequest {
        project_id: project_id.to_string(),
        body: body.to_string(),
        classification: classification.to_string(),
    }
}

/// The rows this test's project actually holds, read through storage rather than through the service.
fn stored(db: &Path, project_id: &str) -> Vec<mayasaba_storage::UserContributionRecord> {
    Storage::open(db)
        .expect("storage")
        .list_user_contributions(project_id)
        .expect("list contributions")
}

#[test]
fn recording_stores_the_advisory_label_and_claims_no_epoch_change() {
    let db = temp_db("record");
    let workspace = temp_workspace("record");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    let outcome = service
        .record_contribution(&record(
            &project_id,
            "Please also cover the export path.",
            "COMMENTARY",
        ))
        .expect("record contribution");

    assert_eq!(outcome.classification, "COMMENTARY");
    assert_eq!(outcome.classification_source, "INTAKE_ROUTER");
    // Nothing routes a contribution to an owning service yet, so the honest outcome is that nothing has ruled.
    assert_eq!(outcome.result_type, "PENDING");
    assert_eq!(outcome.result_reference, None);
    // No confidence was produced, so the field is null rather than an invented number.
    assert_eq!(outcome.classification_confidence, None);
    assert_eq!(outcome.epoch_before, 0);
    assert_eq!(outcome.epoch_after, 0);
    assert_eq!(outcome.created_at, "1700000000");

    // The same facts are what is durable, not only what was returned.
    let rows = stored(&db, &project_id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].contribution_id, outcome.contribution_id);
    assert_eq!(rows[0].body, "Please also cover the export path.");
    assert_eq!(rows[0].result_type, "PENDING");
    assert_eq!(rows[0].epoch_before, rows[0].epoch_after);
}

#[test]
fn a_material_label_is_advisory_and_does_not_advance_the_epoch() {
    let db = temp_db("material");
    let workspace = temp_workspace("material");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    service
        .record_contribution(&record(
            &project_id,
            "Change the target platform to Windows only.",
            "MATERIAL",
        ))
        .expect("record contribution");

    // The label was stored as produced, and it authorised nothing: the service did not act on it, so the
    // outcome is still PENDING and the project's own epoch is untouched. Only the owning authoritative service
    // may advance `project_epoch` (DEC-030).
    let rows = stored(&db, &project_id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].classification, "MATERIAL");
    assert_eq!(rows[0].result_type, "PENDING");

    let project = Storage::open(&db)
        .expect("storage")
        .get_project(&project_id)
        .expect("get project");
    assert_eq!(project.current_epoch, 0);
}

#[test]
fn an_unknown_project_is_refused_and_writes_nothing() {
    let db = temp_db("unknown");
    let workspace = temp_workspace("unknown");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    let error = service
        .record_contribution(&record("prj_does_not_exist", "hello", "COMMENTARY"))
        .expect_err("an unknown project must be refused");

    assert!(
        matches!(error, CreateProjectError::Storage(_)),
        "an unknown project is a storage miss, not a validation failure: {error}"
    );
    // The refusal happened before the write, so nothing was left behind for the project that does exist.
    assert!(stored(&db, &project_id).is_empty());
}

#[test]
fn an_undeclared_classification_is_refused_and_writes_nothing() {
    let db = temp_db("class");
    let workspace = temp_workspace("class");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    let error = service
        .record_contribution(&record(&project_id, "hello", "IMPORTANT"))
        .expect_err("an undeclared classification must be refused");

    match error {
        CreateProjectError::Validation(ProjectValidationError::UnknownClassification(value)) => {
            assert_eq!(value, "IMPORTANT");
        }
        other => panic!("expected an unknown-classification rejection, got {other}"),
    }
    assert!(stored(&db, &project_id).is_empty());
}

#[test]
fn a_whitespace_only_body_is_refused_and_writes_nothing() {
    let db = temp_db("blank");
    let workspace = temp_workspace("blank");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    let error = service
        .record_contribution(&record(&project_id, "   \n\t ", "COMMENTARY"))
        .expect_err("a whitespace-only body must be refused");

    match error {
        CreateProjectError::Validation(ProjectValidationError::EmptyField(name)) => {
            assert_eq!(name, "body");
        }
        other => panic!("expected an empty-field rejection, got {other}"),
    }
    assert!(stored(&db, &project_id).is_empty());
}

#[test]
fn the_stored_body_is_the_trimmed_text_that_was_validated() {
    let db = temp_db("trim");
    let workspace = temp_workspace("trim");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    let outcome = service
        .record_contribution(&record(
            &project_id,
            "  keep the surrounding space out  ",
            "CONTEXT",
        ))
        .expect("record contribution");

    assert_eq!(outcome.body, "keep the surrounding space out");
    assert_eq!(
        stored(&db, &project_id)[0].body,
        "keep the surrounding space out"
    );
}

#[test]
fn two_contributions_are_two_rows_and_the_first_is_never_rewritten() {
    let db = temp_db("append");
    let workspace = temp_workspace("append");
    let project_id = create_project(&db, &workspace);

    let mut service = ProjectService::open(&db)
        .expect("project service")
        .with_fixed_clock("1700000000");

    let first = service
        .record_contribution(&record(&project_id, "first", "COMMENTARY"))
        .expect("record first");
    let second = service
        .record_contribution(&record(&project_id, "second", "COMMENTARY"))
        .expect("record second");

    // Distinct identity, so a retry is distinguishable from a duplicate.
    assert_ne!(first.contribution_id, second.contribution_id);

    // Oldest first, and the earlier row is unchanged by the later one. History is append-only: there is no
    // update path and no delete path (DEC-030, and the same rule DEC-106 states for attachments).
    let rows = stored(&db, &project_id);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].contribution_id, first.contribution_id);
    assert_eq!(rows[0].body, "first");
    assert_eq!(rows[1].contribution_id, second.contribution_id);
    assert_eq!(rows[1].body, "second");
}
