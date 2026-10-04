//! Storage-level tests for project creation.
//!
//! The load-bearing property is that the display name cannot be supplied by a caller. `NewProject` has no
//! `name` field, so the name is derived from the canonical workspace path inside this crate. That closes an
//! injection surface the first implementation had: `NewProject.name` was `pub`, and any crate depending on
//! `mayasaba-storage` could persist a project whose name disagreed with its folder, bypassing the owning
//! service entirely.

use mayasaba_storage::{derive_project_display_name, NewProject, Storage, ROOT_WORKSPACE_NAME};

fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-store-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn new_project(local_path: &std::path::Path, tag: &str) -> NewProject {
    NewProject {
        project_id: format!("prj_{tag}"),
        local_path: local_path.to_string_lossy().into_owned(),
        brief_id: format!("brf_{tag}"),
        brief_body: "Body".to_string(),
        brief_source: "TEST".to_string(),
        event_id: format!("evt_{tag}"),
        created_at: "1700000000".to_string(),
    }
}

#[test]
fn the_stored_name_is_derived_from_the_workspace_folder_not_supplied() {
    let dir = existing_dir("derive").join("InvoiceAI");
    std::fs::create_dir_all(&dir).unwrap();

    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "derive"))
        .expect("create");

    let record = storage.get_project("prj_derive").expect("read back");
    assert_eq!(
        record.name, "InvoiceAI",
        "the persisted name must be the folder's leaf name, because the request has no name field to supply"
    );
    assert_eq!(record.local_path, dir.to_string_lossy());
}

#[test]
fn a_caller_cannot_make_the_name_disagree_with_the_folder() {
    // The attempt is now unrepresentable rather than merely discouraged: there is no field to set. What this
    // test pins is the consequence - the name always tracks the folder.
    let dir = existing_dir("cannot").join("RealFolder");
    std::fs::create_dir_all(&dir).unwrap();

    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&new_project(&dir, "cannot"))
        .expect("create");

    let record = storage.get_project("prj_cannot").expect("read back");
    assert_eq!(record.name, "RealFolder");
    assert_ne!(
        record.name, "SomethingElse",
        "no caller-supplied name can survive"
    );

    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn a_filesystem_root_gets_the_explicit_fallback_label() {
    assert_eq!(derive_project_display_name("C:\\"), ROOT_WORKSPACE_NAME);
    assert_eq!(derive_project_display_name(r"C:\Users"), "Users");
    assert_eq!(
        derive_project_display_name(r"C:\Users\pavan\Projects\InvoiceAI"),
        "InvoiceAI"
    );
}

#[test]
fn creation_remains_atomic_with_the_derived_name() {
    // The derivation happens inside the same transaction as everything else, so a rolled-back creation leaves
    // no project and therefore no derived name behind.
    let dir = existing_dir("atomic");
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .inject_fault_before_insert("project_briefs")
        .expect("trigger");

    assert!(storage
        .create_project(&new_project(&dir, "atomic"))
        .is_err());
    assert_eq!(storage.count("projects").unwrap(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}
