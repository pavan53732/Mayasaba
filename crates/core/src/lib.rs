//! Mayasaba core/controller: cross-subsystem application services.
//!
//! A service owns the meaning of a concept, validates the request against the contract, and delegates
//! persistence to the owning storage crate. It never becomes a second authority for a concept another
//! service owns (AGENTS.md section 6).

pub mod bus_runtime;
pub mod diagnostics;
pub mod project_service;
pub mod recovery_service;

pub use mayasaba_storage::{RecoveryIssue, RecoveryReport};
pub use mayasaba_workspace::{WorkspaceRejection, WorkspaceValidation};
pub use project_service::{
    validate_workspace, CreateProjectError, CreateProjectOutcome, ProjectService,
    ProjectValidationError,
};

pub use recovery_service::{build_runtime_recovery_plan, RuntimeRecoveryAction, RuntimeRecoveryCandidate, RuntimeRecoveryExecutionCandidate, RuntimeRecoveryPlan};
