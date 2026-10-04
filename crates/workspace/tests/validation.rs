//! Workspace candidate validation tests.
//!
//! The load-bearing property is that selecting a folder is not authorizing it: a nonexistent path must never
//! become an authorized workspace, and an authorized workspace must be the canonical path rather than whatever
//! spelling the UI sent.

use mayasaba_workspace::{validate_workspace_candidate, WorkspaceRejection};

/// A real directory that exists for the duration of the test.
fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-ws-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn nonexistent_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mayasaba-ws-missing-{}-{}", std::process::id(), tag))
}

#[test]
fn an_existing_local_folder_is_authorized_and_canonicalized() {
    let dir = existing_dir("ok");
    let result = validate_workspace_candidate(&dir.to_string_lossy()).expect("existing folder is a valid candidate");

    assert!(!result.canonical_path.is_empty());
    assert!(std::path::Path::new(&result.canonical_path).is_absolute(), "the persisted root must be absolute");
    assert!(std::path::Path::new(&result.canonical_path).is_dir(), "the canonical path must resolve to the folder");
    assert_eq!(result.requested_path, dir.to_string_lossy(), "the requested spelling is retained for display");

    // A path spelled with a redundant segment must canonicalize to the same root, so the persisted value
    // cannot be used to smuggle a different spelling past a later comparison.
    let roundabout = dir.join(".").join("..").join(dir.file_name().unwrap());
    let second = validate_workspace_candidate(&roundabout.to_string_lossy()).expect("roundabout path is the same folder");
    assert_eq!(second.canonical_path, result.canonical_path, "canonicalization must collapse redundant segments");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_nonexistent_folder_is_rejected_and_is_never_authorized() {
    let missing = nonexistent_dir("nope");
    let result = validate_workspace_candidate(&missing.to_string_lossy());

    assert_eq!(result.err(), Some(WorkspaceRejection::DoesNotExist));
    assert_eq!(
        WorkspaceRejection::DoesNotExist.code(),
        "WORKSPACE_DOES_NOT_EXIST"
    );

    // Selection must not create anything. If validation ever began creating the folder, this would pass
    // silently and a side effect would hide inside a selection gesture.
    assert!(!missing.exists(), "workspace selection must never create the folder");

    let _ = std::fs::remove_dir_all(&missing);
}

#[test]
fn a_file_is_not_a_workspace() {
    let dir = existing_dir("file");
    let file = dir.join("not-a-folder.txt");
    std::fs::write(&file, b"content").expect("write file");

    assert_eq!(
        validate_workspace_candidate(&file.to_string_lossy()).err(),
        Some(WorkspaceRejection::NotADirectory)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_or_whitespace_candidate_is_rejected() {
    assert_eq!(validate_workspace_candidate("").err(), Some(WorkspaceRejection::Empty));
    assert_eq!(validate_workspace_candidate("   ").err(), Some(WorkspaceRejection::Empty));
    assert_eq!(validate_workspace_candidate("\t\n").err(), Some(WorkspaceRejection::Empty));
}

#[test]
fn a_network_location_is_out_of_scope_rather_than_missing() {
    // Reported as not-local, not as a missing directory: Mayasaba is local-first, so the distinction tells
    // the user their location is unsupported rather than that they mistyped it.
    assert_eq!(
        validate_workspace_candidate(r"\\server\share\folder").err(),
        Some(WorkspaceRejection::NotLocal)
    );
    assert_eq!(
        validate_workspace_candidate("//server/share/folder").err(),
        Some(WorkspaceRejection::NotLocal)
    );
}

#[test]
fn rejections_carry_distinct_machine_readable_codes() {
    let codes = [
        WorkspaceRejection::Empty.code(),
        WorkspaceRejection::DoesNotExist.code(),
        WorkspaceRejection::NotADirectory.code(),
        WorkspaceRejection::NotLocal.code(),
        WorkspaceRejection::NotAccessible.code(),
    ];
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), codes.len(), "each rejection needs its own code so the UI can branch without parsing prose");
    assert!(codes.iter().all(|c| c.starts_with("WORKSPACE_")));
}

#[test]
fn every_rejection_has_a_human_message() {
    for rejection in [
        WorkspaceRejection::Empty,
        WorkspaceRejection::DoesNotExist,
        WorkspaceRejection::NotADirectory,
        WorkspaceRejection::NotLocal,
        WorkspaceRejection::NotAccessible,
    ] {
        assert!(!rejection.to_string().is_empty(), "{rejection:?} needs a user-facing message");
    }
}