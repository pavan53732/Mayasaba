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

/// Persist an observed workspace revision through the canonical storage owner.
pub fn record_revision(storage: &mayasaba_storage::Storage, revision: &mayasaba_storage::NewWorkspaceRevision) -> mayasaba_storage::Result<()> {
    storage.insert_workspace_revision(revision)
}

/// Persist one workspace/integration admission decision. WorkspaceService owns the decision semantics; storage
/// enforces its shape, immutability and supersession chain.
pub fn record_admission(
    storage: &mut mayasaba_storage::Storage,
    admission: &mayasaba_storage::NewAdmission,
) -> mayasaba_storage::Result<mayasaba_storage::AdmissionRecord> {
    storage.insert_admission(admission)
}

/// Persist a recoverable workspace checkpoint. Creating the record never claims that an external filesystem/Git
/// operation succeeded; the caller supplies the observed repository/diff facts.
pub fn create_checkpoint(
    storage: &mayasaba_storage::Storage,
    checkpoint_id: &str,
    project_id: &str,
    workspace_id: &str,
    task_id: Option<&str>,
    agent_id: Option<&str>,
    session_id: Option<&str>,
    epoch: i64,
    kind: &str,
    repository_head: Option<&str>,
    diff_hash: Option<&str>,
    created_at: &str,
) -> mayasaba_storage::Result<()> {
    storage.checkpoint_workspace(
        checkpoint_id, project_id, workspace_id, task_id, agent_id, session_id, epoch, kind,
        repository_head, diff_hash, created_at,
    )
}
