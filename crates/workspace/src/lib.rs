//! Mayasaba workspace boundary: workspaces, checkpoints, integration support.
//!
//! Workspace validation lives here because WorkspaceService owns the workspace concept. Its job is narrow
//! and deliberately conservative: decide whether a *candidate* path may become a project's workspace root,
//! and return the canonical form of that path. It never creates directories, never grants access, and never
//! widens scope beyond the folder it was given.
//!
//! Selection is not authorization. Selecting a folder establishes the user's intended boundary; this crate
//! decides whether that boundary is usable. Task-level permissions remain narrower and policy-controlled
//! (DEC-048).

pub mod validation;

pub use mayasaba_storage::{derive_project_display_name, ROOT_WORKSPACE_NAME};
pub use validation::{validate_workspace_candidate, WorkspaceRejection, WorkspaceValidation};

/// Crate identity, retained for the workspace manifest check.
pub const CRATE_NAME: &str = "mayasaba-workspace";

/// Persist an observed workspace revision through the canonical storage owner.\npub fn record_revision(storage: &mayasaba_storage::Storage, revision: &mayasaba_storage::NewWorkspaceRevision) -> mayasaba_storage::Result<()> {\n    storage.insert_workspace_revision(revision)\n}\n