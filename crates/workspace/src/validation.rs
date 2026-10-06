//! Workspace candidate validation.
//!
//! Kept separate from the crate boundary so the boundary stays a thin declaration of what this crate owns.

use mayasaba_storage::derive_project_display_name;
use std::path::Path;

/// Stable error code the Control Room can branch on without parsing prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRejection {
    /// The candidate was empty or whitespace.
    Empty,
    /// Nothing exists at that path.
    DoesNotExist,
    /// Something exists there, but it is a file rather than a folder.
    NotADirectory,
    /// The path is a network location. Mayasaba is local-first; a UNC path is not a local workspace.
    NotLocal,
    /// The path exists and is a local directory, but the operating system refused to canonicalize it -
    /// typically a permissions denial on a parent directory.
    NotAccessible,
}

impl WorkspaceRejection {
    pub fn code(self) -> &'static str {
        match self {
            WorkspaceRejection::Empty => "WORKSPACE_EMPTY",
            WorkspaceRejection::DoesNotExist => "WORKSPACE_DOES_NOT_EXIST",
            WorkspaceRejection::NotADirectory => "WORKSPACE_NOT_A_DIRECTORY",
            WorkspaceRejection::NotLocal => "WORKSPACE_NOT_LOCAL",
            WorkspaceRejection::NotAccessible => "WORKSPACE_NOT_ACCESSIBLE",
        }
    }
}

impl std::fmt::Display for WorkspaceRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            WorkspaceRejection::Empty => "Choose a local folder for this project.",
            WorkspaceRejection::DoesNotExist => {
                "That folder does not exist. Mayasaba does not create folders during workspace selection."
            }
            WorkspaceRejection::NotADirectory => "That path is a file. A workspace must be a folder.",
            WorkspaceRejection::NotLocal => "That location is not local. Mayasaba operates on local folders only.",
            WorkspaceRejection::NotAccessible => {
                "That folder could not be read. Check permissions and try again."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for WorkspaceRejection {}

/// A validated workspace candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceValidation {
    /// Absolute, normalized path. This is what gets persisted as the project workspace root.
    pub canonical_path: String,
    /// The path the user actually selected, before normalization.
    pub requested_path: String,
    /// Initial human-readable project display name, derived from the canonical folder's leaf name
    /// (DEC-050). The Control Room displays this; it does not derive its own.
    pub derived_project_name: String,
}

/// Whether a path names a network location rather than a local one.
///
/// This is deliberately not `value.starts_with(r"\\")`. On Windows `Path::canonicalize` returns the
/// **verbatim** form, so a perfectly local folder canonicalizes to `\\?\C:\...`. A check that treated every
/// leading `\\` as a UNC share therefore refused a canonical local path as "not local" - a real defect that
/// the attachment resolvability tests caught, because resolvability re-validates a *stored canonical* path
/// where workspace selection had only ever validated the raw user spelling.
///
/// The distinction is what follows the two backslashes:
///
/// - `\\?\` is the verbatim namespace, and `\\?\UNC\server\share` is the verbatim spelling of a UNC share, so
///   that one case is still a network path;
/// - `\\.\` is the device namespace, which is local;
/// - anything else after `\\` is a server name, so it is a UNC share.
///
/// Mayasaba is Windows-local (DEC-004), so a network location is refused as out of scope rather than reported
/// as a missing folder.
fn is_network_path(value: &str) -> bool {
    if value.starts_with("//") {
        return true;
    }
    let Some(rest) = value.strip_prefix(r"\\") else {
        return false;
    };
    if let Some(verbatim) = rest.strip_prefix('?') {
        let verbatim = verbatim.trim_start_matches(['\\', '/']);
        // `get(..3)` rather than slicing: a non-ASCII prefix has no 3-byte boundary and would panic.
        return verbatim
            .get(..3)
            .is_some_and(|p| p.eq_ignore_ascii_case("UNC"));
    }
    !rest.starts_with('.')
}

/// Validate a user-selected folder as a candidate workspace root.
///
/// Order matters. Emptiness first because there is nothing to inspect; locality before existence so a network
/// path is refused as out-of-scope rather than reported as a confusing missing directory; existence before
/// directory-ness because a missing path cannot be a file; accessibility last because canonicalization is the
/// only step that can fail on an otherwise valid folder.
///
/// This function never creates anything. Requiring the folder to exist keeps a side effect out of a selection
/// gesture; creating a directory is a separate decision with its own UX and its own authorization.
pub fn validate_workspace_candidate(
    candidate: &str,
) -> Result<WorkspaceValidation, WorkspaceRejection> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Err(WorkspaceRejection::Empty);
    }

    // UNC and extended-length network prefixes. Mayasaba is Windows-local (DEC-004), so a network location is
    // refused as out of scope rather than as a missing folder.
    if is_network_path(trimmed) {
        return Err(WorkspaceRejection::NotLocal);
    }

    let path = Path::new(trimmed);
    if !path.exists() {
        return Err(WorkspaceRejection::DoesNotExist);
    }
    if !path.is_dir() {
        return Err(WorkspaceRejection::NotADirectory);
    }

    // Canonicalize resolves `..`, relative segments and short names, so the persisted root is unambiguous and a
    // later comparison against it cannot be fooled by a different spelling of the same folder.
    let canonical = path
        .canonicalize()
        .map_err(|_| WorkspaceRejection::NotAccessible)?;
    let canonical_path = canonical.to_string_lossy().into_owned();

    Ok(WorkspaceValidation {
        derived_project_name: derive_project_display_name(&canonical_path),
        canonical_path,
        requested_path: trimmed.to_string(),
    })
}

/// What kind of thing an attachment references.
///
/// This is the filesystem's answer, not the caller's. A request may describe what the user believes they
/// selected, but the kind is established by inspecting the path, so a stored `kind` cannot disagree with what
/// was actually there when the reference was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    File,
    Directory,
}

impl AttachmentKind {
    /// The wire spelling the contract declares.
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentKind::File => "FILE",
            AttachmentKind::Directory => "DIRECTORY",
        }
    }
}

/// Stable error code for a refused attachment path, so the Control Room branches on a code and never on prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentRejection {
    /// The candidate was empty or whitespace.
    Empty,
    /// The path is a network location. An attachment is a local reference (DEC-004).
    NotLocal,
    /// Nothing exists at that path.
    DoesNotExist,
    /// The path exists but could not be canonicalized, so its identity cannot be established.
    NotAccessible,
    /// Something exists there, but it is neither a regular file nor a directory.
    KindUnsupported,
    /// The path resolves outside the authorized workspace scope it was offered against.
    ///
    /// Scope is not widened by attaching (DEC-048): an attachment is context for work the project is already
    /// authorized to do, so a path outside the boundary is refused rather than admitted as a special case.
    OutsideScope,
}

impl AttachmentRejection {
    pub fn code(self) -> &'static str {
        match self {
            AttachmentRejection::Empty => "WORKSPACE_EMPTY",
            AttachmentRejection::NotLocal => "WORKSPACE_NOT_LOCAL",
            AttachmentRejection::DoesNotExist => "ATTACHMENT_SOURCE_MISSING",
            AttachmentRejection::NotAccessible => "WORKSPACE_NOT_ACCESSIBLE",
            AttachmentRejection::KindUnsupported => "ATTACHMENT_KIND_UNSUPPORTED",
            AttachmentRejection::OutsideScope => "ATTACHMENT_NOT_IN_SCOPE",
        }
    }
}

impl std::fmt::Display for AttachmentRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            AttachmentRejection::Empty => "Choose a file or folder to attach.",
            AttachmentRejection::NotLocal => {
                "That location is not local. Mayasaba attaches local files and folders only."
            }
            AttachmentRejection::DoesNotExist => {
                "That file or folder does not exist. Attaching records a reference to something real."
            }
            AttachmentRejection::NotAccessible => {
                "That file or folder could not be read. Check permissions and try again."
            }
            AttachmentRejection::KindUnsupported => {
                "That path is neither a file nor a folder, so it cannot be attached."
            }
            AttachmentRejection::OutsideScope => {
                "That file or folder is outside this project's authorized workspace."
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for AttachmentRejection {}

/// A validated attachment reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPath {
    /// Absolute, normalized path. This is what gets persisted as `source_path`, and it is never rewritten.
    pub canonical_path: String,
    /// The path the user actually selected, before normalization.
    pub requested_path: String,
    /// Established by inspecting the path, not supplied by the caller.
    pub kind: AttachmentKind,
}

/// Validate a user-selected file or folder as a candidate attachment inside an authorized scope.
///
/// This is deliberately *not* `validate_workspace_candidate`: a workspace root must be a directory, while an
/// attachment may be a file or a directory, and an attachment must additionally lie inside the workspace
/// boundary. Reusing the workspace validator here would have made one function answer two different questions.
///
/// It inspects metadata only - existence, kind, and the canonical form - and never opens or reads the file's
/// contents. That is the DEC-106 rule: attaching records a reference, so no content is read, copied, hashed or
/// indexed by this step.
///
/// `authorized_scope` is the canonical workspace root the reference is being offered against. Both sides are
/// canonicalized before the containment test, and containment is compared component-wise rather than as a
/// string prefix, so `C:\work\ab` is not accepted as being inside `C:\work\a`.
pub fn validate_attachment_candidate(
    candidate: &str,
    authorized_scope: &str,
) -> Result<AttachmentPath, AttachmentRejection> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Err(AttachmentRejection::Empty);
    }
    if is_network_path(trimmed) {
        return Err(AttachmentRejection::NotLocal);
    }

    let path = Path::new(trimmed);
    if !path.exists() {
        return Err(AttachmentRejection::DoesNotExist);
    }

    let canonical = path
        .canonicalize()
        .map_err(|_| AttachmentRejection::NotAccessible)?;
    let canonical_path = canonical.to_string_lossy().into_owned();

    // Kind is read from the filesystem after canonicalization, so a symlink is classified by what it points at.
    let kind = if canonical.is_file() {
        AttachmentKind::File
    } else if canonical.is_dir() {
        AttachmentKind::Directory
    } else {
        return Err(AttachmentRejection::KindUnsupported);
    };

    // The scope is canonicalized as well, because the stored scope and the candidate must be compared in the
    // same form. A scope that cannot be canonicalized cannot be checked against, so the reference is refused
    // rather than admitted unchecked.
    let scope = Path::new(authorized_scope.trim())
        .canonicalize()
        .map_err(|_| AttachmentRejection::NotAccessible)?;
    if !canonical.starts_with(&scope) {
        return Err(AttachmentRejection::OutsideScope);
    }

    Ok(AttachmentPath {
        canonical_path,
        requested_path: trimmed.to_string(),
        kind,
    })
}
