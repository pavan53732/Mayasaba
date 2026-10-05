//! Mayasaba task/lease runtime helpers.
//!
//! Task acceptance remains owned by the task domain. This crate composes durable attempt identity, lease fencing,
//! resource admission and recovery operations without creating a second authority for project state.

use mayasaba_storage::{
    NewResourceReservation, NewTaskAttempt, Result, Storage,
};

pub const CRATE_NAME: &str = "mayasaba-tasks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptAuthorization {
    pub attempt_no: i64,
    pub fence_token: i64,
}

/// Create a concrete attempt from an already-authorized lease. Storage re-checks the live lease before insert.
pub fn start_attempt(storage: &Storage, attempt: &NewTaskAttempt) -> Result<AttemptAuthorization> {
    storage.insert_task_attempt(attempt)?;
    Ok(AttemptAuthorization {
        attempt_no: attempt.attempt_no,
        fence_token: attempt.fence_token,
    })
}

/// Advance an attempt with compare-and-swap semantics on its expected state.
pub fn advance_attempt(
    storage: &Storage,
    attempt_id: &str,
    expected_state: &str,
    next_state: &str,
) -> Result<()> {
    storage.transition_task_attempt(attempt_id, expected_state, next_state)
}

/// Validate the lease fence immediately before a material side effect.
pub fn authorize_material_action(
    storage: &Storage,
    attempt_id: &str,
    lease_version: i64,
) -> Result<()> {
    storage.verify_attempt_fence(attempt_id, lease_version)
}

/// Reserve a scarce local resource under the current task lease.
pub fn reserve_resource(
    storage: &Storage,
    reservation: &NewResourceReservation,
) -> Result<()> {
    storage.insert_resource_reservation(reservation)
}

/// Release a held reservation using the same lease fence that acquired it.
pub fn release_resource(
    storage: &Storage,
    reservation_id: &str,
    lease_version: i64,
    released_at: &str,
) -> Result<()> {
    storage.release_resource_reservation(reservation_id, lease_version, released_at)
}

/// Recovery-side cleanup for reservations whose durable deadlines have passed.
pub fn expire_resources(storage: &Storage, now: &str) -> Result<u64> {
    storage.expire_due_resource_reservations(now)
}

/// List attempts whose physical execution still requires reconciliation.
pub fn recoverable_attempts(
    storage: &Storage,
    project_id: &str,
) -> Result<Vec<mayasaba_storage::RecoverableAttempt>> {
    storage.list_recoverable_attempts(project_id)
}
