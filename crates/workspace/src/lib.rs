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
pub fn record_revision(
    storage: &mayasaba_storage::Storage,
    revision: &mayasaba_storage::NewWorkspaceRevision,
) -> mayasaba_storage::Result<()> {
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
        checkpoint_id,
        project_id,
        workspace_id,
        task_id,
        agent_id,
        session_id,
        epoch,
        kind,
        repository_head,
        diff_hash,
        created_at,
    )
}

/// Deterministic Git command specifications. The workspace crate defines Git operation semantics but never
/// materializes a process; callers execute the returned ProcessSpec through crates/execution.
pub mod git {
    use mayasaba_execution::ProcessSpec;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct GitWorktreePlan {
        pub repository: String,
        pub worktree: String,
        pub branch: String,
        pub base_ref: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GitWorkspaceError {
        InvalidRepository(String),
        InvalidWorktree(String),
        InvalidBranch(String),
        InvalidRef(String),
    }

    impl std::fmt::Display for GitWorkspaceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::InvalidRepository(v) => write!(f, "invalid Git repository path: {v}"),
                Self::InvalidWorktree(v) => write!(f, "invalid Git worktree path: {v}"),
                Self::InvalidBranch(v) => write!(f, "invalid Git branch name: {v}"),
                Self::InvalidRef(v) => write!(f, "invalid Git base ref: {v}"),
            }
        }
    }

    impl std::error::Error for GitWorkspaceError {}

    fn is_absolute_windows(value: &str) -> bool {
        let b = value.as_bytes();
        (b.len() >= 3 && b[1] == b':' && matches!(b[2], b'\\' | b'/')) || value.starts_with(r"\\")
    }

    fn valid_path(value: &str) -> bool {
        !value.trim().is_empty() && is_absolute_windows(value.trim())
    }

    pub fn status_spec(repository: &str) -> Result<ProcessSpec, GitWorkspaceError> {
        if !valid_path(repository) {
            return Err(GitWorkspaceError::InvalidRepository(repository.to_owned()));
        }
        Ok(ProcessSpec::new(
            "git",
            vec![
                "-C".into(),
                repository.into(),
                "status".into(),
                "--porcelain=v1".into(),
                "--untracked-files=all".into(),
            ],
            repository.to_owned(),
        ))
    }

    pub fn head_spec(repository: &str) -> Result<ProcessSpec, GitWorkspaceError> {
        if !valid_path(repository) {
            return Err(GitWorkspaceError::InvalidRepository(repository.to_owned()));
        }
        Ok(ProcessSpec::new(
            "git",
            vec![
                "-C".into(),
                repository.into(),
                "rev-parse".into(),
                "HEAD".into(),
            ],
            repository.to_owned(),
        ))
    }

    pub fn list_worktrees_spec(repository: &str) -> Result<ProcessSpec, GitWorkspaceError> {
        if !valid_path(repository) {
            return Err(GitWorkspaceError::InvalidRepository(repository.to_owned()));
        }
        Ok(ProcessSpec::new(
            "git",
            vec![
                "-C".into(),
                repository.into(),
                "worktree".into(),
                "list".into(),
                "--porcelain".into(),
            ],
            repository.to_owned(),
        ))
    }

    pub fn add_worktree_spec(plan: &GitWorktreePlan) -> Result<ProcessSpec, GitWorkspaceError> {
        if !valid_path(&plan.repository) {
            return Err(GitWorkspaceError::InvalidRepository(
                plan.repository.clone(),
            ));
        }
        if !valid_path(&plan.worktree) {
            return Err(GitWorkspaceError::InvalidWorktree(plan.worktree.clone()));
        }
        if plan.branch.trim().is_empty() || plan.branch.starts_with('-') {
            return Err(GitWorkspaceError::InvalidBranch(plan.branch.clone()));
        }
        if plan.base_ref.trim().is_empty() || plan.base_ref.starts_with('-') {
            return Err(GitWorkspaceError::InvalidRef(plan.base_ref.clone()));
        }

        Ok(ProcessSpec::new(
            "git",
            vec![
                "-C".into(),
                plan.repository.clone(),
                "worktree".into(),
                "add".into(),
                "-b".into(),
                plan.branch.clone(),
                plan.worktree.clone(),
                plan.base_ref.clone(),
            ],
            plan.repository.clone(),
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn windows_paths_are_validated_without_host_os_assumptions() {
            assert!(valid_path(r"C:\repo"));
            assert!(valid_path(r"D:/workspace"));
            assert!(!valid_path("repo"));
            assert!(!valid_path("/tmp/repo"));
        }

        #[test]
        fn git_worktree_add_is_argument_bound() {
            let plan = GitWorktreePlan {
                repository: r"C:\repo".into(),
                worktree: r"C:\work\agent-1".into(),
                branch: "agent/session-1".into(),
                base_ref: "HEAD".into(),
            };
            let spec = add_worktree_spec(&plan).expect("plan");
            assert_eq!(spec.executable, "git");
            assert_eq!(spec.cwd, r"C:\repo");
            assert_eq!(
                spec.argv,
                vec![
                    "-C",
                    r"C:\repo",
                    "worktree",
                    "add",
                    "-b",
                    "agent/session-1",
                    r"C:\work\agent-1",
                    "HEAD"
                ]
            );
            assert!(!spec
                .argv
                .iter()
                .any(|arg| arg.contains("&&") || arg.contains('|')));
        }

        #[test]
        fn option_like_git_arguments_are_refused() {
            let mut plan = GitWorktreePlan {
                repository: r"C:\repo".into(),
                worktree: r"C:\work\agent-1".into(),
                branch: "--delete".into(),
                base_ref: "HEAD".into(),
            };
            assert!(add_worktree_spec(&plan).is_err());
            plan.branch = "agent/session-1".into();
            plan.base_ref = "--orphan".into();
            assert!(add_worktree_spec(&plan).is_err());
        }
    }
}
