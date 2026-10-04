//! Vertical slice tests: project intake end to end through storage.
//!
//! These are the first tests in the repository that exercise real behaviour rather than contract shape. The
//! architectural promise is atomic creation - a project never exists without its intent anchor - so the
//! rollback test is the load-bearing one. It injects a genuine SQLite fault with a trigger rather than a
//! mock, so the rollback is performed by the database and cannot be asserted away.

use mayasaba_core::project_service::{
    CreateProjectError, CreateProjectOutcome, CreateProjectRequest, ProjectService, ProjectValidationError,
};

fn request() -> CreateProjectRequest {
    CreateProjectRequest {
        name: "Mayasaba".to_string(),
        local_path: r"C:\work\mayasaba".to_string(),
        initial_brief_body: "Build a local-first control plane for coordinating CLI coding agents.".to_string(),
        brief_source: None,
    }
}

fn created_project(outcome: CreateProjectOutcome) -> mayasaba_storage::ProjectRecord {
    match outcome {
        CreateProjectOutcome::Created { project } => project,
    }
}

#[test]
fn creates_project_brief_epoch_and_event_atomically() {
    let mut service = ProjectService::in_memory().expect("service").with_fixed_clock("1700000000");

    let project = created_project(service.create_project(&request()).expect("creation should succeed"));

    // Authoritative readback: the record carries the brief, so the anchor exists.
    assert_eq!(project.name, "Mayasaba");
    assert_eq!(project.phase, "DISCOVERY");
    assert_eq!(project.status, "ACTIVE");
    assert_eq!(project.current_epoch, 0, "a new project starts at epoch 0");
    assert_eq!(project.brief_version, Some(1), "the first brief version is 1");
    assert!(project.brief_id.is_some(), "a project cannot exist without a brief id");
    assert_eq!(
        project.brief_body.as_deref(),
        Some("Build a local-first control plane for coordinating CLI coding agents.")
    );

    // All four writes are present, and only one of each.
    let storage = service.storage();
    assert_eq!(storage.count("projects").unwrap(), 1);
    assert_eq!(storage.count("project_briefs").unwrap(), 1);
    assert_eq!(storage.count("project_epochs").unwrap(), 1);
    assert_eq!(storage.count("events").unwrap(), 1);
}

#[test]
fn a_project_never_exists_without_its_brief() {
    // The brief insert is made to fail by a real SQLite trigger, after the project row has already been
    // written inside the transaction. If the four writes were not atomic, the project row would survive.
    let mut service = ProjectService::in_memory().expect("service").with_fixed_clock("1700000000");
    service.storage().inject_fault_before_insert("project_briefs").expect("install fault trigger");

    let result = service.create_project(&request());

    assert!(
        result.is_err(),
        "creation must fail when the brief cannot be written, because the anchor is mandatory"
    );

    let storage = service.storage();
    assert_eq!(storage.count("projects").unwrap(), 0, "project row must be rolled back");
    assert_eq!(storage.count("project_briefs").unwrap(), 0);
    assert_eq!(storage.count("project_epochs").unwrap(), 0);
    assert_eq!(storage.count("events").unwrap(), 0, "no event may survive a rolled-back creation");
}

#[test]
fn a_blank_initial_brief_is_rejected_before_anything_is_persisted() {
    let mut service = ProjectService::in_memory().expect("service").with_fixed_clock("1700000000");

    for blank in ["", "   ", "\t\n  \t"] {
        let mut bad = request();
        bad.initial_brief_body = blank.to_string();
        let result = service.create_project(&bad);

        match result {
            Err(CreateProjectError::Validation(ProjectValidationError::BlankInitialBrief)) => {}
            other => panic!("whitespace brief {blank:?} must be rejected as BlankInitialBrief, got {other:?}"),
        }
    }

    assert_eq!(service.storage().count("projects").unwrap(), 0, "a rejected request must not persist anything");
    assert_eq!(service.storage().count("project_briefs").unwrap(), 0);
}

#[test]
fn empty_name_and_path_are_rejected() {
    let mut service = ProjectService::in_memory().expect("service").with_fixed_clock("1700000000");

    let mut no_name = request();
    no_name.name = "  ".to_string();
    assert!(matches!(
        service.create_project(&no_name),
        Err(CreateProjectError::Validation(ProjectValidationError::EmptyField("name")))
    ));

    let mut no_path = request();
    no_path.local_path = "".to_string();
    assert!(matches!(
        service.create_project(&no_path),
        Err(CreateProjectError::Validation(ProjectValidationError::EmptyField("local_path")))
    ));

    assert_eq!(service.storage().count("projects").unwrap(), 0);
}

#[test]
fn reading_back_an_unknown_project_is_not_found_rather_than_an_empty_record() {
    let service = ProjectService::in_memory().expect("service");
    let err = service.storage().get_project("prj_does_not_exist").expect_err("must not resolve");
    assert!(
        matches!(err, mayasaba_storage::StorageError::NotFound(_)),
        "an unknown project must be NotFound, not a default record, or the Control Room could render a project that does not exist"
    );
}

#[test]
fn two_projects_may_share_a_local_path_because_the_contract_does_not_forbid_it() {
    // Recorded deliberately. Nothing in the architecture or the schema makes local_path unique, so this
    // test asserts the *current* rule rather than inventing one. Whether two projects may target the same
    // workspace is an undecided product question and needs a contract decision, not a constraint added
    // quietly in a test.
    let mut service = ProjectService::in_memory().expect("service").with_fixed_clock("1700000001");

    service.create_project(&request()).expect("first project");
    let mut second = request();
    second.name = "Second".to_string();
    second.local_path = r"C:\work\mayasaba".to_string();
    service.create_project(&second).expect("second project on the same path is currently permitted");

    assert_eq!(service.storage().count("projects").unwrap(), 2);
}

#[test]
fn creation_survives_reopening_the_database_from_disk() {
    // Proves the durable claim rather than an in-memory artefact: the project and its brief are read back
    // from a file-backed database in a new service instance.
    let dir = std::env::temp_dir().join(format!("mayasaba-slice-{}", std::process::id()));
    let db = dir.join("mayasaba.sqlite3");
    let _ = std::fs::remove_file(&db);

    let project_id = {
        let mut service = ProjectService::open(&db).expect("open").with_fixed_clock("1700000002");
        let project = created_project(service.create_project(&request()).expect("create"));
        project.project_id.clone()
    };

    let reopened = ProjectService::open(&db).expect("reopen");
    let record = reopened.storage().get_project(&project_id).expect("read back after reopen");
    assert_eq!(record.brief_version, Some(1), "the brief survives a reopen");
    assert_eq!(record.current_epoch, 0);

    let _ = std::fs::remove_dir_all(&dir);
}