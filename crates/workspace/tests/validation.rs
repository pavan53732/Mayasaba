//! Workspace candidate validation tests.
//!
//! The load-bearing property is that selecting a folder is not authorizing it: a nonexistent path must never
//! become an authorized workspace, and an authorized workspace must be the canonical path rather than whatever
//! spelling the UI sent.

use mayasaba_workspace::{
    derive_project_display_name, validate_workspace_candidate, WorkspaceRejection,
    ROOT_WORKSPACE_NAME,
};

/// A real directory that exists for the duration of the test.
fn existing_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mayasaba-ws-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn nonexistent_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "mayasaba-ws-missing-{}-{}",
        std::process::id(),
        tag
    ))
}

#[test]
fn an_existing_local_folder_is_authorized_and_canonicalized() {
    let dir = existing_dir("ok");
    let result = validate_workspace_candidate(&dir.to_string_lossy())
        .expect("existing folder is a valid candidate");

    assert!(!result.canonical_path.is_empty());
    assert!(
        std::path::Path::new(&result.canonical_path).is_absolute(),
        "the persisted root must be absolute"
    );
    assert!(
        std::path::Path::new(&result.canonical_path).is_dir(),
        "the canonical path must resolve to the folder"
    );
    assert_eq!(
        result.requested_path,
        dir.to_string_lossy(),
        "the requested spelling is retained for display"
    );

    // A path spelled with a redundant segment must canonicalize to the same root, so the persisted value
    // cannot be used to smuggle a different spelling past a later comparison.
    let roundabout = dir.join(".").join("..").join(dir.file_name().unwrap());
    let second = validate_workspace_candidate(&roundabout.to_string_lossy())
        .expect("roundabout path is the same folder");
    assert_eq!(
        second.canonical_path, result.canonical_path,
        "canonicalization must collapse redundant segments"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_canonicalized_workspace_root_is_still_local() {
    // Regression test for the locality classifier. On Windows a canonical path is `\\?\C:\...`, so a check
    // that treated every leading `\\` as a UNC share refused a canonical local folder as "not local". This is
    // reachable: a previously canonicalized workspace root may be re-validated.
    let dir = existing_dir("ws-verbatim");
    let canonical = std::fs::canonicalize(&dir).expect("canonicalize");

    let validated = validate_workspace_candidate(&canonical.to_string_lossy())
        .expect("a canonical local folder is a valid workspace root");
    assert!(!validated.canonical_path.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_verbatim_unc_workspace_root_is_still_refused_as_remote() {
    assert_eq!(
        validate_workspace_candidate(r"\\?\UNC\server\share\folder").err(),
        Some(WorkspaceRejection::NotLocal),
        "the verbatim spelling of a UNC share is still a network location"
    );
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
    assert!(
        !missing.exists(),
        "workspace selection must never create the folder"
    );

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
    assert_eq!(
        validate_workspace_candidate("").err(),
        Some(WorkspaceRejection::Empty)
    );
    assert_eq!(
        validate_workspace_candidate("   ").err(),
        Some(WorkspaceRejection::Empty)
    );
    assert_eq!(
        validate_workspace_candidate("\t\n").err(),
        Some(WorkspaceRejection::Empty)
    );
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
    assert_eq!(
        sorted.len(),
        codes.len(),
        "each rejection needs its own code so the UI can branch without parsing prose"
    );
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
        assert!(
            !rejection.to_string().is_empty(),
            "{rejection:?} needs a user-facing message"
        );
    }
}
#[test]
fn the_display_name_is_derived_from_the_folder_leaf() {
    // Nested under the temp dir so the leaf really is `InvoiceAI` and not the helper's own prefixed name.
    let parent = existing_dir("derived");
    let dir = parent.join("InvoiceAI");
    std::fs::create_dir_all(&dir).unwrap();

    let result = validate_workspace_candidate(&dir.to_string_lossy()).expect("valid candidate");
    assert_eq!(
        result.derived_project_name, "InvoiceAI",
        "the display name is the folder's own leaf name"
    );
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn a_filesystem_root_falls_back_rather_than_inventing_a_name() {
    // `C:\` has no folder name of its own. The rule is a small explicit fallback, not a naming subsystem,
    // and it must not pretend the fallback is the folder's name.
    assert_eq!(derive_project_display_name(r"C:\\"), ROOT_WORKSPACE_NAME);
    assert_eq!(derive_project_display_name("C:\\"), ROOT_WORKSPACE_NAME);
    // A folder one level below the root still has a real name.
    assert_eq!(derive_project_display_name(r"C:\Users"), "Users");
    assert_eq!(
        derive_project_display_name(r"C:\Users\Pavan\Projects\InvoiceAI"),
        "InvoiceAI"
    );
}

#[test]
fn the_display_name_is_metadata_and_never_an_identity_key() {
    // Two projects in differently-located folders that share a leaf name must both be creatable and must
    // remain distinguishable by project_id. This is why no UNIQUE constraint on name or local_path is added.
    let one = existing_dir("alpha").join("Shared");
    let two = existing_dir("beta").join("Shared");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();

    let a = validate_workspace_candidate(&one.to_string_lossy()).expect("first candidate");
    let b = validate_workspace_candidate(&two.to_string_lossy()).expect("second candidate");
    assert_eq!(
        a.derived_project_name, b.derived_project_name,
        "the same leaf name is allowed twice"
    );
    assert_ne!(
        a.canonical_path, b.canonical_path,
        "the workspaces are genuinely different folders"
    );

    let _ = std::fs::remove_dir_all(&one);
    let _ = std::fs::remove_dir_all(&two);
}
