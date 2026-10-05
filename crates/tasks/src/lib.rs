//! Mayasaba task/lease runtime helpers.
//!
//! Task acceptance remains owned by the task domain. This crate composes durable attempt identity, lease fencing,
//! resource admission and recovery operations without creating a second authority for project state.

use mayasaba_storage::{
    NewResourceReservation, NewTaskAttempt, NewTaskLease, Result, Storage,
};

pub const CRATE_NAME: &str = "mayasaba-tasks";

/// Deterministically select READY tasks whose dependency and live-lease gates are already satisfied.
/// TaskService still owns the subsequent lease admission; selection itself performs no mutation.
pub fn select_schedulable_tasks(
    storage: &Storage,
    project_id: &str,
    limit: usize,
) -> Result<Vec<mayasaba_storage::SchedulableTask>> {
    storage.list_schedulable_tasks(project_id, limit)
}


/// Admit one task lease and atomically advance REQUESTED -> ACTIVE.
pub fn lease_task(storage: &mut Storage, lease: &NewTaskLease) -> Result<mayasaba_storage::TaskLeaseRecord> {
    storage.lease_task(lease)
}

/// Renew a lease using the current fence version; storage increments the version atomically.
pub fn renew_lease(
    storage: &mut Storage,
    lease_id: &str,
    expected_version: i64,
    heartbeat_at: &str,
    expires_at: &str,
) -> Result<mayasaba_storage::TaskLeaseRecord> {
    storage.renew_lease(lease_id, expected_version, heartbeat_at, expires_at)
}

/// Release a live lease with an optimistic fencing check.
pub fn release_lease(
    storage: &mut Storage,
    lease_id: &str,
    expected_version: i64,
    now: &str,
) -> Result<()> {
    storage.release_lease(lease_id, expected_version, now)
}

/// Revoke a live lease with an optimistic fencing check.
pub fn revoke_lease(
    storage: &mut Storage,
    lease_id: &str,
    expected_version: i64,
    now: &str,
) -> Result<()> {
    storage.revoke_lease(lease_id, expected_version, now)
}

/// Expire due leases and persist the owning Task's LEASE_EXPIRED consequence atomically.
pub fn expire_leases(storage: &mut Storage, now: &str) -> Result<u64> {
    storage.expire_due_leases(now)
}

/// Return a lease-expired Task to READY only after durable attempt/process reconciliation is clear.
pub fn recover_expired_task(storage: &mut Storage, task_id: &str, now: &str) -> Result<()> {
    storage.recover_expired_task(task_id, now)
}


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
    ended_at: Option<&str>,
) -> Result<()> {
    storage.transition_task_attempt(attempt_id, expected_state, next_state, ended_at)
}

/// Validate the lease fence immediately before a material side effect.
pub fn authorize_material_action(
    storage: &Storage,
    attempt_id: &str,
    lease_version: i64,
) -> Result<()> {
    storage.verify_attempt_authority(attempt_id, lease_version)
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


/// Refresh the durable heartbeat under the current lease fence.
pub fn heartbeat_attempt(
    storage: &Storage,
    attempt_id: &str,
    lease_version: i64,
    heartbeat_at: &str,
) -> Result<()> {
    storage.heartbeat_task_attempt(attempt_id, lease_version, heartbeat_at)
}
