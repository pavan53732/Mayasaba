//! Attachment path validation tests.
//!
//! The load-bearing property is that an attachment is a reference *inside the project's authorized scope*.
//! Selecting a file is not authorizing it, and attaching never widens the boundary: a path outside the
//! workspace is refused even though it exists and is perfectly readable.
//!
//! The containment test is component-wise rather than a string prefix, and the sibling-prefix test below is
//! what proves it: `C:\work\ab` must not be accepted as being inside `C:\work\a`.

use mayasaba_workspace::{validate_attachment_candidate, AttachmentKind, AttachmentRejection};

/// A real directory that exists for the duration of the test, unique per call so parallel tests cannot
/// remove each other's fixtures.
fn temp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mayasaba-attach-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn a_file_inside_the_scope_is_accepted_and_typed_by_the_filesystem() {
    let scope = temp_dir("file-ok");
    let file = scope.join("notes.txt");
    std::fs::write(&file, b"supporting context").expect("write file");

    let validated =
        validate_attachment_candidate(&file.to_string_lossy(), &scope.to_string_lossy())
            .expect("a file inside the scope is attachable");

    assert_eq!(validated.kind, AttachmentKind::File);
    assert_eq!(validated.kind.as_str(), "FILE");
    assert!(
        std::path::Path::new(&validated.canonical_path).is_absolute(),
        "the persisted reference must be absolute"
    );
    assert_eq!(
        validated.requested_path,
        file.to_string_lossy(),
        "the requested spelling is retained for display"
    );

    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn a_directory_inside_the_scope_is_accepted_as_a_directory() {
    let scope = temp_dir("dir-ok");
    let nested = scope.join("docs");
    std::fs::create_dir_all(&nested).expect("create nested dir");

    let validated =
        validate_attachment_candidate(&nested.to_string_lossy(), &scope.to_string_lossy())
            .expect("a directory inside the scope is attachable");

    assert_eq!(validated.kind, AttachmentKind::Directory);
    assert_eq!(validated.kind.as_str(), "DIRECTORY");

    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn the_workspace_root_itself_may_be_attached_as_a_directory() {
    // The boundary is inclusive: a user may attach the project folder itself, and that means "the scope",
    // not a snapshot of its contents.
    let scope = temp_dir("root-ok");
    let validated =
        validate_attachment_candidate(&scope.to_string_lossy(), &scope.to_string_lossy())
            .expect("the scope root is inside the scope");

    assert_eq!(validated.kind, AttachmentKind::Directory);

    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn a_path_outside_the_scope_is_refused_even_though_it_exists() {
    let scope = temp_dir("outside-scope");
    let outside = temp_dir("outside-elsewhere");
    let file = outside.join("secret.txt");
    std::fs::write(&file, b"outside").expect("write file");

    assert_eq!(
        validate_attachment_candidate(&file.to_string_lossy(), &scope.to_string_lossy()).err(),
        Some(AttachmentRejection::OutsideScope),
        "existence is not authorization: scope is not widened by attaching"
    );
    assert_eq!(
        AttachmentRejection::OutsideScope.code(),
        "ATTACHMENT_NOT_IN_SCOPE"
    );

    let _ = std::fs::remove_dir_all(&scope);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn a_sibling_whose_name_shares_a_prefix_is_not_inside_the_scope() {
    // The defect this guards: a string-prefix containment test would accept `...\scopeSibling` as being
    // inside `...\scope`. Containment is compared component-wise, so it does not.
    let parent = temp_dir("prefix");
    let scope = parent.join("a");
    let sibling = parent.join("ab");
    std::fs::create_dir_all(&scope).expect("create scope");
    std::fs::create_dir_all(&sibling).expect("create sibling");
    let file = sibling.join("file.txt");
    std::fs::write(&file, b"not in scope").expect("write file");

    assert_eq!(
        validate_attachment_candidate(&file.to_string_lossy(), &scope.to_string_lossy()).err(),
        Some(AttachmentRejection::OutsideScope),
        "a name prefix is not containment"
    );

    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn a_nonexistent_source_is_refused_and_nothing_is_created() {
    let scope = temp_dir("missing");
    let missing = scope.join("does-not-exist.txt");

    assert_eq!(
        validate_attachment_candidate(&missing.to_string_lossy(), &scope.to_string_lossy()).err(),
        Some(AttachmentRejection::DoesNotExist)
    );
    assert_eq!(
        AttachmentRejection::DoesNotExist.code(),
        "ATTACHMENT_SOURCE_MISSING"
    );
    assert!(
        !missing.exists(),
        "attaching must never create the thing it references"
    );

    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn an_empty_candidate_is_refused_before_anything_is_inspected() {
    let scope = temp_dir("empty");
    for candidate in ["", "   ", "\t\n"] {
        assert_eq!(
            validate_attachment_candidate(candidate, &scope.to_string_lossy()).err(),
            Some(AttachmentRejection::Empty)
        );
    }
    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn a_network_location_is_refused_as_out_of_scope_rather_than_missing() {
    let scope = temp_dir("unc");
    for candidate in [r"\\server\share\file.txt", "//server/share/file.txt"] {
        assert_eq!(
            validate_attachment_candidate(candidate, &scope.to_string_lossy()).err(),
            Some(AttachmentRejection::NotLocal),
            "a network path is not a local attachment (DEC-004)"
        );
    }
    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn a_canonicalized_local_path_is_not_mistaken_for_a_network_path() {
    // Regression test for a real defect. On Windows `Path::canonicalize` returns the verbatim form
    // (`\\?\C:\...`), and a locality check that treated every leading `\\` as a UNC share refused a canonical
    // local path as "not local". Workspace selection never noticed, because it only ever validated the raw
    // user spelling - but resolvability re-validates the *stored canonical* path, so every stored attachment
    // resolved as unresolved.
    let scope = temp_dir("verbatim");
    let file = scope.join("file.txt");
    std::fs::write(&file, b"content").expect("write file");

    let canonical = std::fs::canonicalize(&file).expect("canonicalize the file");
    let canonical_scope = std::fs::canonicalize(&scope).expect("canonicalize the scope");

    let validated = validate_attachment_candidate(
        &canonical.to_string_lossy(),
        &canonical_scope.to_string_lossy(),
    )
    .expect("a canonical local path is local");

    assert_eq!(validated.kind, AttachmentKind::File);

    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn a_verbatim_unc_path_is_still_a_network_path() {
    // Accepting verbatim paths must not become a way to smuggle a network location past the locality rule:
    // `\\?\UNC\server\share` is the verbatim spelling of a UNC share.
    let scope = temp_dir("verbatim-unc");
    assert_eq!(
        validate_attachment_candidate(r"\\?\UNC\server\share\file.txt", &scope.to_string_lossy())
            .err(),
        Some(AttachmentRejection::NotLocal)
    );
    let _ = std::fs::remove_dir_all(&scope);
}

#[test]
fn attachment_rejections_carry_distinct_machine_readable_codes() {
    let codes = [
        AttachmentRejection::Empty.code(),
        AttachmentRejection::NotLocal.code(),
        AttachmentRejection::DoesNotExist.code(),
        AttachmentRejection::NotAccessible.code(),
        AttachmentRejection::KindUnsupported.code(),
        AttachmentRejection::OutsideScope.code(),
    ];
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        codes.len(),
        "each refusal needs its own code so the UI can branch without parsing prose"
    );
    // The two attachment-specific refusals are namespaced to the concept; the rest reuse the workspace
    // vocabulary rather than restating it, because locality and accessibility are the workspace's questions.
    assert!(codes.contains(&"ATTACHMENT_NOT_IN_SCOPE"));
    assert!(codes.contains(&"ATTACHMENT_KIND_UNSUPPORTED"));
}

#[test]
fn every_attachment_rejection_has_a_human_message() {
    for rejection in [
        AttachmentRejection::Empty,
        AttachmentRejection::NotLocal,
        AttachmentRejection::DoesNotExist,
        AttachmentRejection::NotAccessible,
        AttachmentRejection::KindUnsupported,
        AttachmentRejection::OutsideScope,
    ] {
        assert!(
            !rejection.to_string().is_empty(),
            "{rejection:?} needs a user-facing message"
        );
    }
}
