//! Mayasaba local execution boundary.
//!
//! Process execution is controller-mediated. These helpers establish the mandatory lease-fence precondition
//! before the execution implementation performs a material command.

use mayasaba_storage::{Result, Storage};

pub const CRATE_NAME: &str = "mayasaba-execution";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionAuthorization {
    pub attempt_id: String,
    pub lease_version: i64,
}

/// Admission hook required immediately before a material execution starts.
pub fn authorize_execution(storage: &Storage, attempt_id: &str, lease_version: i64) -> Result<ExecutionAuthorization> {
    storage.verify_attempt_fence(attempt_id, lease_version)?;
    Ok(ExecutionAuthorization { attempt_id: attempt_id.to_string(), lease_version })
}
