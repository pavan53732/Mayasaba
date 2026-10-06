//! Attachment slice tests: attach, list and resolve end to end through durable storage.
//!
//! These exercise the properties DEC-106 makes load-bearing rather than the shape of the API:
//!
//! - attaching records a reference and its provenance, and computes no content identity;
//! - the scope a reference is checked against comes from the project, never from the caller;
//! - a re-attachment is a new identity, because history is append-only;
//! - resolvability is a check performed when read, so a deleted or moved source leaves the row in place and
//!   is reported as an outcome rather than rendered as though no attachment existed;
//! - a modified source still resolves, because content identity is not implied by an attachment.

use std::path::{Path, PathBuf};

use mayasaba_core::attachment_service::{
    AttachRequest, AttachmentError, AttachmentProvenance, AttachmentService, ResolvabilityVerdict,
};
use mayasaba_core::project_service::{CreateProjectOutcome, CreateProjectRequest, ProjectService};
use mayasaba_storage::StorageError;

/// A unique database file per call, so parallel tests in one process cannot share durable state.
fn temp_db(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let db = std::env::temp_dir().join(format!(
        "mayasaba-attach-db-{}-{}-{}.sqlite",
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
        "mayasaba-attach-ws-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create workspace dir");
    dir
}

/// Create a real project through the owning service, so the attachment has a genuine project row.
fn create_project(db: &Path, workspace: &Path) -> String {
    let mut service = ProjectService::open(db).expect("project service");
    let outcome = service
        .create_project(&CreateProjectRequest {
            local_path: workspace.to_string_lossy().into_owned(),
            initial_brief_body: "Attachment slice test project.".to_string(),
            brief_source: None,
        })
        .expect("create project");
    match outcome {
        CreateProjectOutcome::Created { project } => project.project_id,
    }
}

fn attach_request(
    project_id: &str,
    source: &Path,
    provenance: AttachmentProvenance,
) -> AttachRequest {
    AttachRequest {
        project_id: project_id.to_string(),
        source_path: source.to_string_lossy().into_owned(),
        provenance,
    }
}

#[test]
fn attach_records_a_reference_with_provenance_and_no_content_identity() {
    let db = temp_db("record");
    let workspace = temp_workspace("record");
    let project_id = create_project(&db, &workspace);
    let file = workspace.join("requirements.md");
    std::fs::write(&file, b"supporting context").expect("write file");

    let mut service = AttachmentService::open(&db)
        .expect("attachment service")
        .with_fixed_clock("1700000000");

    let resolution = service
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::InitialIntakeComposer,
        ))
        .expect("attach succeeds");

    let record = &resolution.attachment;
    assert!(record.attachment_id.starts_with("att_"));
    assert_eq!(record.project_id, project_id);
    assert_eq!(record.kind, "FILE");
    assert_eq!(record.provenance, "INITIAL_INTAKE_COMPOSER");
    assert_eq!(
        record.lifecycle_state, "SELECTED",
        "a fresh reference is selected and not yet consumed"
    );
    assert_eq!(
        record.content_hash, None,
        "attaching computes no content hash; capture is a separate explicit operation"
    );
    assert_eq!(
        record.context_evidence_id, None,
        "attaching links no evidence; consume is a separate explicit operation"
    );
    assert_eq!(record.captured_at, "1700000000");
    assert!(
        Path::new(&record.source_path).is_absolute(),
        "the stored reference is the canonical path"
    );
    assert_eq!(
        record.authorized_scope,
        Path::new(&workspace)
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "the scope recorded is the project's own workspace root"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn attach_never_reads_the_file_contents() {
    // A binary file that is not valid UTF-8, and that no text reader could open. Attaching it must succeed,
    // because attaching records a reference: a read, a copy or a hash on this path would fail here.
    let db = temp_db("binary");
    let workspace = temp_workspace("binary");
    let project_id = create_project(&db, &workspace);
    let file = workspace.join("blob.bin");
    std::fs::write(&file, [0xff, 0xfe, 0x00, 0x01, 0x80]).expect("write binary file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let resolution = service
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("a binary file is attachable");

    assert_eq!(resolution.attachment.content_hash, None);
    assert_eq!(resolution.attachment.kind, "FILE");

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_directory_is_attached_as_a_scope_and_not_as_a_snapshot() {
    let db = temp_db("directory");
    let workspace = temp_workspace("directory");
    let project_id = create_project(&db, &workspace);
    let nested = workspace.join("docs");
    std::fs::create_dir_all(&nested).expect("create nested dir");
    std::fs::write(nested.join("a.md"), b"one").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let resolution = service
        .attach(&attach_request(
            &project_id,
            &nested,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("attach succeeds");

    assert_eq!(resolution.attachment.kind, "DIRECTORY");

    // The reference is to the directory itself. Adding a file afterwards does not change the attachment,
    // because no enumeration was ever stored.
    std::fs::write(nested.join("b.md"), b"two").expect("write second file");
    let again = service
        .resolve(&project_id, &resolution.attachment.attachment_id)
        .expect("resolve succeeds");
    assert_eq!(again.verdict, ResolvabilityVerdict::Resolved);

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn the_scope_is_read_from_the_project_and_never_from_the_caller() {
    let db = temp_db("scope");
    let workspace = temp_workspace("scope");
    let elsewhere = temp_workspace("scope-elsewhere");
    let project_id = create_project(&db, &workspace);
    let outside = elsewhere.join("outside.txt");
    std::fs::write(&outside, b"outside the boundary").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let refused = service.attach(&attach_request(
        &project_id,
        &outside,
        AttachmentProvenance::ChatComposer,
    ));

    assert!(
        matches!(
            refused,
            Err(AttachmentError::Workspace(
                mayasaba_workspace::AttachmentRejection::OutsideScope
            ))
        ),
        "a path outside the project's workspace must be refused, got {refused:?}"
    );

    // Nothing was persisted by the refusal.
    assert!(service
        .list_resolutions(&project_id)
        .expect("list")
        .is_empty());

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&elsewhere);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn re_attaching_the_same_path_is_a_new_identity_rather_than_an_update() {
    let db = temp_db("reattach");
    let workspace = temp_workspace("reattach");
    let project_id = create_project(&db, &workspace);
    let file = workspace.join("spec.md");
    std::fs::write(&file, b"content").expect("write file");

    let mut first = AttachmentService::open(&db)
        .expect("attachment service")
        .with_fixed_clock("1700000001");
    let one = first
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::InitialIntakeComposer,
        ))
        .expect("first attach");

    let mut second = AttachmentService::open(&db)
        .expect("attachment service")
        .with_fixed_clock("1700000002");
    let two = second
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("second attach");

    assert_ne!(
        one.attachment.attachment_id, two.attachment.attachment_id,
        "history is append-only, so a re-attachment is a new identity"
    );
    assert_eq!(
        one.attachment.source_path, two.attachment.source_path,
        "the same path may be referenced twice; the table declares no uniqueness on it"
    );

    let listed = second.list_resolutions(&project_id).expect("list");
    assert_eq!(
        listed.len(),
        2,
        "both selections remain visible rather than the second replacing the first"
    );
    assert_eq!(
        listed[0].attachment.provenance, "INITIAL_INTAKE_COMPOSER",
        "the list reads oldest first, so provenance shows which surface offered each reference"
    );
    assert_eq!(listed[1].attachment.provenance, "CHAT_COMPOSER");

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_live_reference_resolves_with_every_check_passing() {
    let db = temp_db("live");
    let workspace = temp_workspace("live");
    let project_id = create_project(&db, &workspace);
    let file = workspace.join("live.txt");
    std::fs::write(&file, b"here").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let attached = service
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("attach");

    let resolved = service
        .resolve(&project_id, &attached.attachment.attachment_id)
        .expect("resolve");

    assert_eq!(resolved.verdict, ResolvabilityVerdict::Resolved);
    assert_eq!(resolved.checks.len(), 4);
    assert!(resolved.checks.iter().all(|c| c.status == "PASS"));
    assert!(resolved.checks.iter().all(|c| c.code.is_none()));
    let names: Vec<&str> = resolved.checks.iter().map(|c| c.check).collect();
    assert_eq!(names, vec!["EXISTS", "LOCALITY", "KIND", "SCOPE"]);

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_deleted_source_leaves_the_row_and_is_reported_as_an_outcome() {
    let db = temp_db("deleted");
    let workspace = temp_workspace("deleted");
    let project_id = create_project(&db, &workspace);
    let file = workspace.join("gone.txt");
    std::fs::write(&file, b"temporary").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let attached = service
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("attach");

    std::fs::remove_file(&file).expect("delete the source");

    let resolved = service
        .resolve(&project_id, &attached.attachment.attachment_id)
        .expect("resolve still answers; the row is durable");
    assert_eq!(resolved.verdict, ResolvabilityVerdict::Unresolved);
    assert_eq!(resolved.checks[0].check, "EXISTS");
    assert_eq!(resolved.checks[0].status, "FAIL");
    assert_eq!(
        resolved.checks[0].code,
        Some("ATTACHMENT_SOURCE_MISSING"),
        "the failure is recorded with a typed code rather than rendered as an empty attachment"
    );
    assert!(
        resolved.checks[1..]
            .iter()
            .all(|c| c.status == "NOT_APPLICABLE"),
        "checks the validator never reached are reported as not applicable, not as passing"
    );

    // The row persists: history is not rewritten because the filesystem changed.
    let listed = service.list_resolutions(&project_id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].verdict, ResolvabilityVerdict::Unresolved);

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_modified_source_still_resolves_because_content_identity_is_not_implied() {
    let db = temp_db("modified");
    let workspace = temp_workspace("modified");
    let project_id = create_project(&db, &workspace);
    let file = workspace.join("changing.txt");
    std::fs::write(&file, b"first revision").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let attached = service
        .attach(&attach_request(
            &project_id,
            &file,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("attach");

    std::fs::write(&file, b"second revision, entirely different").expect("rewrite file");

    let resolved = service
        .resolve(&project_id, &attached.attachment.attachment_id)
        .expect("resolve");
    assert_eq!(
        resolved.verdict,
        ResolvabilityVerdict::Resolved,
        "modification does not invalidate an attachment: it is context, not truth"
    );
    assert_eq!(
        resolved.attachment.content_hash, None,
        "no hash was computed, so nothing claims the content is unchanged"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_moved_source_is_not_rewritten_into_the_row() {
    let db = temp_db("moved");
    let workspace = temp_workspace("moved");
    let project_id = create_project(&db, &workspace);
    let original = workspace.join("before.txt");
    let moved = workspace.join("after.txt");
    std::fs::write(&original, b"payload").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let attached = service
        .attach(&attach_request(
            &project_id,
            &original,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("attach");
    let stored_path = attached.attachment.source_path.clone();

    std::fs::rename(&original, &moved).expect("move the source");

    let resolved = service
        .resolve(&project_id, &attached.attachment.attachment_id)
        .expect("resolve");
    assert_eq!(resolved.verdict, ResolvabilityVerdict::Unresolved);
    assert_eq!(
        resolved.attachment.source_path, stored_path,
        "source_path is never rewritten; rewriting it would rewrite history"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn resolving_an_unknown_attachment_reports_not_found() {
    let db = temp_db("unknown");
    let workspace = temp_workspace("unknown");
    let project_id = create_project(&db, &workspace);

    let service = AttachmentService::open(&db).expect("attachment service");
    let result = service.resolve(&project_id, "att_does_not_exist");

    assert!(
        matches!(
            result,
            Err(AttachmentError::Storage(StorageError::NotFound(_)))
        ),
        "an attachment row is never deleted, so a missing identity means it was never recorded"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn attaching_to_an_unknown_project_is_refused_before_validation() {
    let db = temp_db("noproject");
    let workspace = temp_workspace("noproject");
    let file = workspace.join("orphan.txt");
    std::fs::write(&file, b"orphan").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let result = service.attach(&attach_request(
        "prj_missing",
        &file,
        AttachmentProvenance::ChatComposer,
    ));

    // The missing project surfaces as `NotFound` rather than `UnknownProject`, and the order is the reason:
    // the service reads the project to obtain the scope it must validate against, so the read fails before the
    // insert that would have reported `UnknownProject`. Either way nothing is persisted, which is the property
    // that matters; the command layer maps both onto one registered code.
    assert!(
        matches!(
            result,
            Err(AttachmentError::Storage(StorageError::NotFound(_)))
        ),
        "an attachment must belong to a real project, got {result:?}"
    );
    assert!(
        file.exists(),
        "the referenced file is untouched by the refusal"
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn a_blank_source_path_is_refused_as_a_request_field() {
    let db = temp_db("blank");
    let workspace = temp_workspace("blank");
    let project_id = create_project(&db, &workspace);

    let mut service = AttachmentService::open(&db).expect("attachment service");
    let result = service.attach(&AttachRequest {
        project_id,
        source_path: "   ".to_string(),
        provenance: AttachmentProvenance::ChatComposer,
    });

    assert!(matches!(
        result,
        Err(AttachmentError::EmptyField("source_path"))
    ));

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn provenance_distinguishes_the_two_composer_surfaces() {
    // Both surfaces must be able to attach, and the recorded provenance must say which one did. A single
    // surface would satisfy neither the requirement nor this test.
    let db = temp_db("provenance");
    let workspace = temp_workspace("provenance");
    let project_id = create_project(&db, &workspace);
    let intake_file = workspace.join("intake.txt");
    let chat_file = workspace.join("chat.txt");
    std::fs::write(&intake_file, b"intake").expect("write file");
    std::fs::write(&chat_file, b"chat").expect("write file");

    let mut service = AttachmentService::open(&db).expect("attachment service");
    service
        .attach(&attach_request(
            &project_id,
            &intake_file,
            AttachmentProvenance::InitialIntakeComposer,
        ))
        .expect("intake surface can attach");
    service
        .attach(&attach_request(
            &project_id,
            &chat_file,
            AttachmentProvenance::ChatComposer,
        ))
        .expect("chat surface can attach");

    assert_eq!(
        AttachmentProvenance::InitialIntakeComposer.as_str(),
        "INITIAL_INTAKE_COMPOSER"
    );
    assert_eq!(AttachmentProvenance::ChatComposer.as_str(), "CHAT_COMPOSER");

    let provenances: Vec<String> = service
        .list_resolutions(&project_id)
        .expect("list")
        .into_iter()
        .map(|r| r.attachment.provenance)
        .collect();
    assert_eq!(
        provenances,
        vec!["INITIAL_INTAKE_COMPOSER", "CHAT_COMPOSER"]
    );

    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_file(&db);
}
