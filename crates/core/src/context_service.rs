//! ContextService owns immutable ContextPack snapshots and freshness decisions.
//!
//! Context snapshots are inputs to work, never mutable shared state. A project epoch change invalidates old
//! snapshots; historical rows remain intact for audit and replay.

use mayasaba_storage::{ContextSnapshotRecord, NewContextSnapshot, Result, Storage};

pub struct ContextService {
    storage: Storage,
}

impl ContextService {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn storage_mut(&mut self) -> &mut Storage {
        &mut self.storage
    }

    pub fn create_snapshot(
        &mut self,
        snapshot: &NewContextSnapshot,
    ) -> Result<ContextSnapshotRecord> {
        self.storage.create_context_snapshot(snapshot)
    }

    /// Material project-truth change: increment the epoch and invalidate all older contexts transactionally.
    pub fn advance_epoch(
        &mut self,
        project_id: &str,
        reason: &str,
        now: &str,
    ) -> Result<mayasaba_storage::ProjectEpochAdvance> {
        self.storage.advance_project_epoch(project_id, reason, now)
    }

    pub fn is_fresh(
        &self,
        context_snapshot_id: &str,
        project_id: &str,
        epoch: i64,
        state_digest: &str,
    ) -> Result<bool> {
        self.storage
            .validate_context_fresh(context_snapshot_id, project_id, epoch, state_digest)
    }

    pub fn supersede(&mut self, context_snapshot_id: &str, at: &str) -> Result<()> {
        self.storage
            .supersede_context_snapshot(context_snapshot_id, at)
    }

    pub fn invalidate(&mut self, context_snapshot_id: &str, at: &str) -> Result<()> {
        self.storage
            .invalidate_context_snapshot(context_snapshot_id, at)
    }
}
