//! AttachmentService: the owning service for `ProjectContextAttachment` (DEC-107).
//!
//! An attachment is a durable local reference with captured provenance (DEC-106). This service owns the
//! entity, its association with a project, and its lifecycle. It deliberately does not own the two things the
//! entity depends on: `WorkspaceService` decides whether a path is local and inside the authorized scope
//! (DEC-048), and `EvidenceService` owns capture, content hashing and evidence provenance (DEC-102). Both
//! boundaries are *used* here rather than reimplemented, so there is one authority per question.
//!
//! Three operations exist and are deliberately not collapsed (DEC-106):
//!
//! 1. **attach** - record the reference and its provenance. Implemented here.
//! 2. **capture** - compute a content hash and produce an Artifact/Evidence record. Owned by EvidenceService,
//!    explicit and on request only; it is not wired yet, and nothing on the attach path performs it.
//! 3. **consume** - an owning service accepts a material change caused by the contents. Explicit and separate.
//!
//! Attaching therefore reads no content. It inspects metadata to establish locality, scope and kind, and
//! stores a reference; the absence of a hash on this path is the point, not an omission.

use std::path::Path;

use mayasaba_storage::{
    NewProjectContextAttachment, ProjectContextAttachmentRecord, Storage, StorageError,
};
use mayasaba_workspace::{validate_attachment_candidate, AttachmentPath, AttachmentRejection};

use crate::project_service::{next_nonce, ProjectService};

/// Where an attachment was offered from.
///
/// The two surfaces are the Initial Intake Composer and the Ongoing Chat Composer (DEC-106). Recording which
/// one produced a reference is provenance, and provenance is never mutated afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentProvenance {
    InitialIntakeComposer,
    ChatComposer,
}

impl AttachmentProvenance {
    /// The wire spelling the contract declares.
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentProvenance::InitialIntakeComposer => "INITIAL_INTAKE_COMPOSER",
            AttachmentProvenance::ChatComposer => "CHAT_COMPOSER",
        }
    }
}

/// A request to attach one local file or folder to a project.
///
/// There is no `authorized_scope` field, and that is a boundary rather than an omission: the scope is read
/// from the project's own stored workspace root by this service. A caller-supplied scope would let the caller
/// choose the boundary its own path is checked against, which is the one thing scope validation exists to
/// prevent (DEC-048).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachRequest {
    pub project_id: String,
    pub source_path: String,
    pub provenance: AttachmentProvenance,
}

/// The verdict of one resolvability evaluation.
///
/// Resolvability is evaluated **when read** and never stored (DEC-106): a path that was resolvable yesterday
/// and is not today has not changed the attachment, only the observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvabilityVerdict {
    Resolved,
    Unresolved,
}

impl ResolvabilityVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            ResolvabilityVerdict::Resolved => "RESOLVED",
            ResolvabilityVerdict::Unresolved => "UNRESOLVED",
        }
    }
}

/// One named check inside a resolvability evaluation.
///
/// This reuses the `Admission` shape the repository already has - a per-check status plus exactly one verdict
/// (DEC-106, `schemas/workspace-v1/admission.schema.json`) - rather than introducing a second check-outcome
/// vocabulary. The status vocabulary is the admission schema's own: `PASS`, `FAIL`, `NOT_APPLICABLE`, where
/// `NOT_APPLICABLE` means the check was not reached because an earlier one already decided the verdict. That
/// is honest about what was actually evaluated; reporting an unevaluated check as `PASS` would not be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentCheck {
    /// `EXISTS`, `LOCALITY`, `KIND` or `SCOPE`.
    pub check: &'static str,
    /// `PASS`, `FAIL` or `NOT_APPLICABLE`.
    pub status: &'static str,
    /// The registered error code when the check failed; `None` when it passed or was not reached.
    pub code: Option<&'static str>,
    pub detail: Option<String>,
}

/// One attachment plus the resolvability evaluation performed when it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentResolution {
    pub attachment: ProjectContextAttachmentRecord,
    pub verdict: ResolvabilityVerdict,
    pub checks: Vec<AttachmentCheck>,
}

/// The checks, in the order they are evaluated. The order is the dependency order of the questions: there is
/// nothing to ask about locality or scope until something exists, and nothing to ask about scope until the
/// path resolves to a kind.
const CHECKS: [&str; 4] = ["EXISTS", "LOCALITY", "KIND", "SCOPE"];

pub struct AttachmentService {
    storage: Storage,
    // Send + Sync because Tauri managed state requires it, exactly as ProjectService does.
    now: Box<dyn Fn() -> String + Send + Sync>,
}

impl AttachmentService {
    /// Open the durable store at `path` and apply the canonical schema.
    pub fn open(path: &Path) -> Result<Self, AttachmentError> {
        Ok(AttachmentService {
            storage: Storage::open(path)?,
            now: Box::new(epoch_seconds),
        })
    }

    /// Ephemeral store for tests.
    pub fn in_memory() -> Result<Self, AttachmentError> {
        Ok(AttachmentService {
            storage: Storage::open_in_memory()?,
            now: Box::new(epoch_seconds),
        })
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

    /// Record an attachment reference.
    ///
    /// Validation runs to completion before storage is touched, so a refused path cannot leave a row behind.
    /// The stored `source_path` is the canonical form, and the stored `authorized_scope` is the project's own
    /// workspace root rather than whatever the caller believed it to be.
    ///
    /// A re-attachment of the same path is deliberately a **new identity**, not an update: `source_path` is
    /// never rewritten and a row is never deleted, so history shows that the path was selected twice rather
    /// than pretending the first selection did not happen (DEC-106).
    pub fn attach(&mut self, req: &AttachRequest) -> Result<AttachmentResolution, AttachmentError> {
        if req.project_id.trim().is_empty() {
            return Err(AttachmentError::EmptyField("project_id"));
        }
        if req.source_path.trim().is_empty() {
            return Err(AttachmentError::EmptyField("source_path"));
        }

        // The project's stored workspace root is the scope. It is read here rather than accepted from the
        // caller so a request cannot widen the boundary it is validated against.
        //
        // A missing project is reported as its own variant rather than left as the store's `NotFound`. The only
        // row that read can fail to find is the project itself, and `NotFound` also means "no such attachment"
        // on the other two operations - so passing it through would give one code two meanings that no caller
        // could tell apart (DEC-055).
        let project = self
            .storage
            .get_project(&req.project_id)
            .map_err(|e| match e {
                StorageError::NotFound(_) => AttachmentError::UnknownProject {
                    project_id: req.project_id.clone(),
                },
                other => AttachmentError::Storage(other),
            })?;

        // Locality, existence, kind and scope are WorkspaceService's authority (DEC-107). A refusal is
        // returned as its own typed rejection; this service does not reinterpret it.
        let validated = validate_attachment_candidate(&req.source_path, &project.local_path)?;

        let nonce = next_nonce();
        let captured_at = (self.now)();
        let new = NewProjectContextAttachment {
            attachment_id: format!(
                "att_{}",
                ProjectService::digest(&format!("attachment:{nonce}"))
            ),
            project_id: req.project_id.clone(),
            source_path: validated.canonical_path,
            kind: validated.kind.as_str().to_string(),
            authorized_scope: project.local_path.clone(),
            provenance: req.provenance.as_str().to_string(),
            captured_at,
        };

        let record = self.storage.insert_project_context_attachment(&new)?;
        Ok(Self::resolution_of(record))
    }

    /// Every attachment of a project, each with a resolvability evaluation.
    ///
    /// The evaluation is performed per row on read, so the list the Control Room renders cannot present a
    /// stale resolvability verdict as though it were current state.
    ///
    /// This reads rows and does not require the project to exist: an unknown project id yields an empty list
    /// rather than an error, because the question "which attachment rows name this project?" has the truthful
    /// answer "none". `attach` does require the project, because it writes an FK-constrained row.
    pub fn list_resolutions(
        &self,
        project_id: &str,
    ) -> Result<Vec<AttachmentResolution>, AttachmentError> {
        let records = self.storage.list_project_context_attachments(project_id)?;
        Ok(records.into_iter().map(Self::resolution_of).collect())
    }

    /// One attachment with a fresh resolvability evaluation.
    pub fn resolve(
        &self,
        project_id: &str,
        attachment_id: &str,
    ) -> Result<AttachmentResolution, AttachmentError> {
        let record = self
            .storage
            .get_project_context_attachment(project_id, attachment_id)?;
        Ok(Self::resolution_of(record))
    }

    /// Evaluate one stored reference against the scope it was recorded under.
    ///
    /// The decision procedure is `WorkspaceService`'s validator, called with the row's own `authorized_scope`
    /// rather than the project's current workspace root. Reusing the validator keeps one implementation of the
    /// rules, and using the recorded scope means the check answers "is this reference still resolvable as it
    /// was recorded?" rather than "would it be accepted today?".
    fn resolution_of(record: ProjectContextAttachmentRecord) -> AttachmentResolution {
        let outcome = validate_attachment_candidate(&record.source_path, &record.authorized_scope);
        let (checks, verdict) = Self::checks_for(outcome);
        AttachmentResolution {
            attachment: record,
            verdict,
            checks,
        }
    }

    /// Map the validator's outcome onto the check list and the one verdict.
    ///
    /// The validator stops at the first failure, so the checks after the failing one were not evaluated and are
    /// reported `NOT_APPLICABLE` rather than `PASS`. A modified source still resolves: content identity is not
    /// implied by an attachment, so modification does not invalidate it. A deleted or moved source fails
    /// `EXISTS`, and the row stays in place with that outcome recorded (DEC-106).
    fn checks_for(
        outcome: Result<AttachmentPath, AttachmentRejection>,
    ) -> (Vec<AttachmentCheck>, ResolvabilityVerdict) {
        let passed = |check: &'static str| AttachmentCheck {
            check,
            status: "PASS",
            code: None,
            detail: None,
        };
        let skipped = |check: &'static str| AttachmentCheck {
            check,
            status: "NOT_APPLICABLE",
            code: None,
            detail: None,
        };

        match outcome {
            Ok(_) => (
                CHECKS.iter().map(|c| passed(c)).collect(),
                ResolvabilityVerdict::Resolved,
            ),
            Err(rejection) => {
                // Which check the rejection belongs to, and how far the evaluation got. Empty, missing and
                // inaccessible all mean the same thing to a reader: the reference does not resolve to
                // anything usable, and the code distinguishes why.
                let (failed_index, code) = match rejection {
                    AttachmentRejection::Empty => (0, "WORKSPACE_EMPTY"),
                    AttachmentRejection::DoesNotExist => (0, "ATTACHMENT_SOURCE_MISSING"),
                    AttachmentRejection::NotAccessible => (0, "WORKSPACE_NOT_ACCESSIBLE"),
                    AttachmentRejection::NotLocal => (1, "WORKSPACE_NOT_LOCAL"),
                    AttachmentRejection::KindUnsupported => (2, "ATTACHMENT_KIND_UNSUPPORTED"),
                    AttachmentRejection::OutsideScope => (3, "ATTACHMENT_NOT_IN_SCOPE"),
                };

                let checks = CHECKS
                    .iter()
                    .enumerate()
                    .map(|(index, check)| match index.cmp(&failed_index) {
                        std::cmp::Ordering::Less => passed(check),
                        std::cmp::Ordering::Equal => AttachmentCheck {
                            check,
                            status: "FAIL",
                            code: Some(code),
                            detail: Some(rejection.to_string()),
                        },
                        std::cmp::Ordering::Greater => skipped(check),
                    })
                    .collect();

                (checks, ResolvabilityVerdict::Unresolved)
            }
        }
    }
}

/// RFC3339-shaped timestamp without pulling a date dependency into the slice, matching ProjectService.
fn epoch_seconds() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

#[derive(Debug)]
pub enum AttachmentError {
    /// A required request field was blank, refused before persistence.
    EmptyField(&'static str),
    /// The named project does not exist. An attachment cannot outlive its project.
    UnknownProject {
        project_id: String,
    },
    /// The path was refused by WorkspaceService's locality/scope validator.
    Workspace(AttachmentRejection),
    Storage(StorageError),
}

impl From<AttachmentRejection> for AttachmentError {
    fn from(e: AttachmentRejection) -> Self {
        AttachmentError::Workspace(e)
    }
}

impl From<StorageError> for AttachmentError {
    fn from(e: StorageError) -> Self {
        AttachmentError::Storage(e)
    }
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachmentError::EmptyField(name) => write!(f, "{name} must not be empty"),
            AttachmentError::UnknownProject { project_id } => {
                write!(f, "project {project_id} does not exist")
            }
            AttachmentError::Workspace(e) => write!(f, "{e}"),
            AttachmentError::Storage(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AttachmentError {}
