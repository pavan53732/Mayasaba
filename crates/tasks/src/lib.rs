//! Mayasaba task/lease runtime helpers.
//!
//! Task acceptance remains owned by the task domain. This crate only composes the durable storage primitives
//! needed to distinguish task identity from a concrete retry attempt and to fence stale leases.

use mayasaba_storage::{NewTaskAttempt, Result, Storage};

pub const CRATE_NAME: &str = "mayasaba-tasks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptAuthorization {
    pub attempt_no: i64,
    pub fence_token: i64,
}

/// Create a concrete attempt from an already-authorized lease. Storage re-checks the live lease before insert.
pub fn start_attempt(storage: &Storage, attempt: &NewTaskAttempt) -> Result<AttemptAuthorization> {
    storage.insert_task_attempt(attempt)?;
    Ok(AttemptAuthorization { attempt_no: attempt.attempt_no, fence_token: attempt.fence_token })
}

/// Validate the lease fence immediately before a material side effect.
pub fn authorize_material_action(storage: &Storage, attempt_id: &str, lease_version: i64) -> Result<()> {
    storage.verify_attempt_fence(attempt_id, lease_version)
}
