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


/// Validate a user-selected folder as a candidate workspace root.
///
/// Order matters. Emptiness first because there is nothing to inspect; locality before existence so a network
/// path is refused as out-of-scope rather than reported as a confusing missing directory; existence before
/// directory-ness because a missing path cannot be a file; accessibility last because canonicalization is the
/// only step that can fail on an otherwise valid folder.
///
/// This function never creates anything. Requiring the folder to exist keeps a side effect out of a selection
/// gesture; creating a directory is a separate decision with its own UX and its own authorization.
pub fn validate_workspace_candidate(candidate: &str) -> Result<WorkspaceValidation, WorkspaceRejection> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Err(WorkspaceRejection::Empty);
    }

    // UNC and extended-length network prefixes. Mayasaba is Windows-local (DEC-004), so a network location is
    // refused as out of scope rather than as a missing folder.
    if trimmed.starts_with(r"\\") || trimmed.starts_with("//") {
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
    let canonical = path.canonicalize().map_err(|_| WorkspaceRejection::NotAccessible)?;
    let canonical_path = canonical.to_string_lossy().into_owned();

    Ok(WorkspaceValidation {
        derived_project_name: derive_project_display_name(&canonical_path),
        canonical_path,
        requested_path: trimmed.to_string(),
    })
}
