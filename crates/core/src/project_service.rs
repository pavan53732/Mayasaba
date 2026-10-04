//! ProjectService: the owning service for project creation and the ProjectBrief intent anchor.
//!
//! This is the vertical slice's domain boundary. It does three things and no more:
//!
//! 1. validates the incoming request against the declared contract, before anything is persisted;
//! 2. mints the identifiers a creation needs;
//! 3. delegates the atomic write to `mayasaba-storage` and returns authoritative state.
//!
//! It deliberately does not decide what constitutes *meaningful* project intent. The structural rule - the
//! brief is present and not whitespace - is enforced here and in
//! `schemas/tauri-bridge-v1/payload-types.json`. Whether the text is a real project is the intake router's
//! judgement, and a schema must not pretend to make it.

use std::path::Path;

use mayasaba_storage::{CreatedProject, NewProject, ProjectRecord, Storage};
use mayasaba_workspace::{validate_workspace_candidate, WorkspaceRejection, WorkspaceValidation};

/// Validate a user-selected folder as a candidate workspace root, without persisting anything.
///
/// This is the step that makes selection an authorization act rather than a string. The Control Room calls it
/// when the user picks a folder and shows the outcome; `create_project` validates again, so a caller that
/// skips this still cannot persist an unchecked path.
pub fn validate_workspace(candidate: &str) -> Result<WorkspaceValidation, WorkspaceRejection> {
    validate_workspace_candidate(candidate)
}

/// Request as declared by `create_projectRequest` in `schemas/tauri-bridge-v1/payload-types.json`.
///
/// There is no `name` field. The display name is derived by the owning service from the validated canonical
/// workspace path, so the UI cannot supply identity metadata that disagrees with the filesystem (DEC-050).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectRequest {
    pub local_path: String,
    pub initial_brief_body: String,
    pub brief_source: Option<String>,
}

/// A request that failed the contract before any persistence was attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectValidationError {
    EmptyField(&'static str),
    /// The brief was whitespace only. The machine contract rejects this structurally with
    /// `minLength: 1` and `pattern: \S`; this is the runtime enforcement of the same rule.
    BlankInitialBrief,
    /// Reserved for the domain judgement that intake owns, deliberately not taken here.
    IntentNotYetAssessed,
}

impl std::fmt::Display for ProjectValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectValidationError::EmptyField(name) => write!(f, "{name} must not be empty"),
            ProjectValidationError::BlankInitialBrief => {
                write!(f, "initial_brief.body must contain project intent, not whitespace")
            }
            ProjectValidationError::IntentNotYetAssessed => {
                write!(f, "brief intake assessment is not implemented")
            }
        }
    }
}

impl std::error::Error for ProjectValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateProjectOutcome {
    Created { project: ProjectRecord },
}

/// Owns its store rather than borrowing one. Borrowing `&mut Storage` for the service's lifetime made the
/// store unusable by the caller for the whole time the service existed, which is a sign the ownership is
/// backwards: a service owns the store it writes through.
pub struct ProjectService {
    storage: Storage,
    // Send + Sync because Tauri managed state requires it. rusqlite::Connection is Send but not Sync, which
    // is why the desktop shell holds the service behind a Mutex rather than sharing it directly.
    now: Box<dyn Fn() -> String + Send + Sync>,
}

impl ProjectService {
    /// Open the durable store at `path` and apply the canonical schema.
    pub fn open(path: &Path) -> Result<Self, CreateProjectError> {
        Ok(ProjectService { storage: Storage::open(path)?, now: Box::new(epoch_seconds) })
    }

    /// Ephemeral store for tests.
    pub fn in_memory() -> Result<Self, CreateProjectError> {
        Ok(ProjectService { storage: Storage::open_in_memory()?, now: Box::new(epoch_seconds) })
    }

    /// Replace the clock. For tests that assert on stored timestamps.
    pub fn with_fixed_clock(mut self, stamp: &'static str) -> Self {
        self.now = Box::new(move || stamp.to_string());
        self
    }

    /// Read-only access to the store, for assertions and for the caller that owns the lifecycle.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Validate, mint identifiers, and create the project with its brief, epoch and event atomically.
    ///
    /// Validation runs to completion before storage is touched, so a rejected request cannot leave a
    /// partial project behind. That is the cheap half of atomicity; the durable half is that the four
    /// writes share one transaction inside the storage crate.
    pub fn create_project(&mut self, req: &CreateProjectRequest) -> Result<CreateProjectOutcome, CreateProjectError> {
        Self::validate(req)?;

        // The workspace is re-validated here, not trusted from the caller. The Control Room validates a
        // candidate when the user selects it so it can show feedback, but a client that skips that step must
        // still not be able to persist an unchecked path. Only the canonical form is persisted, so the stored
        // workspace root is the normalized path rather than whatever spelling the UI happened to send.
        let workspace = validate_workspace_candidate(&req.local_path)?;

        let created_at = (self.now)();
        let nonce = next_nonce();

        let new = NewProject {
            project_id: format!("prj_{}", Self::digest(&format!("project:{nonce}"))),
            // Derived by the owning service from the canonical workspace folder name. The caller supplies no
            // name, so the UI cannot invent identity metadata that disagrees with the filesystem.
            name: workspace.derived_project_name.clone(),
            local_path: workspace.canonical_path,
            brief_id: format!("brf_{}", Self::digest(&format!("brief:{nonce}"))),
            brief_body: req.initial_brief_body.trim().to_string(),
            brief_source: req.brief_source.clone().unwrap_or_else(|| "INITIAL_INTAKE_COMPOSER".to_string()),
            event_id: format!("evt_{}", Self::digest(&format!("event:{nonce}"))),
            created_at: created_at.clone(),
        };

        let CreatedProject { project_id, .. } = self.storage.create_project(&new)?;
        let record = self.storage.get_project(&project_id)?;
        Ok(CreateProjectOutcome::Created { project: record })
    }

    /// The structural half of the contract. A request that fails here never reaches persistence.
    pub fn validate(req: &CreateProjectRequest) -> Result<(), ProjectValidationError> {
        if req.local_path.trim().is_empty() {
            return Err(ProjectValidationError::EmptyField("local_path"));
        }
        if req.initial_brief_body.trim().is_empty() {
            return Err(ProjectValidationError::BlankInitialBrief);
        }
        Ok(())
    }

    /// Deterministic digest of an identity seed.
    ///
    /// Identity is *not* derived from request content. An earlier version seeded ids from
    /// `(local_path, created_at)`, which collided for two projects on the same path created in the same
    /// second - a genuine unsound-identity bug that the duplicate-path test caught. Content-derived ids also
    /// make a retry indistinguishable from a duplicate. Uniqueness is enforced by the PRIMARY KEY, so this
    /// only has to be collision-resistant, not secure.
    fn digest(seed: &str) -> String {
        // FNV-1a, kept dependency-free for the slice. Replaced by the SHA-256 helper when ids become
        // externally referenced or security-relevant.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in seed.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
        format!("{hash:016x}")
    }
}

/// Per-process entropy plus a monotonic counter.
///
/// `RandomState` is seeded by the operating system, so this yields cross-process uniqueness without adding
/// a uuid dependency. The counter guarantees distinctness within the process even if two creations land in
/// the same clock tick.
fn next_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(n);
    format!("{:016x}", hasher.finish())
}

/// RFC3339-shaped timestamp without pulling a date dependency into the slice. Replaced by a real clock
/// implementation when evidence ordering depends on sub-second precision.
fn epoch_seconds() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

#[derive(Debug)]
pub enum CreateProjectError {
    Validation(ProjectValidationError),
    Workspace(WorkspaceRejection),
    Storage(mayasaba_storage::StorageError),
}

impl From<WorkspaceRejection> for CreateProjectError {
    fn from(e: WorkspaceRejection) -> Self {
        CreateProjectError::Workspace(e)
    }
}

impl From<ProjectValidationError> for CreateProjectError {
    fn from(e: ProjectValidationError) -> Self {
        CreateProjectError::Validation(e)
    }
}

impl From<mayasaba_storage::StorageError> for CreateProjectError {
    fn from(e: mayasaba_storage::StorageError) -> Self {
        CreateProjectError::Storage(e)
    }
}

impl std::fmt::Display for CreateProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateProjectError::Validation(e) => write!(f, "{e}"),
            CreateProjectError::Workspace(e) => write!(f, "{e}"),
            CreateProjectError::Storage(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CreateProjectError {}