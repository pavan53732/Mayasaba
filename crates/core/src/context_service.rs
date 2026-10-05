//! ContextService owns immutable ContextPack snapshots and freshness decisions.
//!
//! Context snapshots are inputs to work, never mutable shared state. A project epoch change invalidates old
//! snapshots; historical rows remain intact for audit and replay.

use mayasaba_storage::{
    ContextSnapshotRecord, NewContextSnapshot, NewEvent, Result, Storage,
};

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
        &self,
        snapshot: &NewContextSnapshot,
    ) -> Result<ContextSnapshotRecord> {
        let record = self.storage.create_context_snapshot(snapshot)?;
        self.emit_event(
            &record,
            "CONTEXT_CREATED",
            &format!(
                r#"{{"context_snapshot_id":"{}","project_id":"{}","epoch":{},"state_digest":"{}"}}"#,
                record.context_snapshot_id, record.project_id, record.epoch, record.state_digest
            ),
            &record.created_at,
        )?;
        Ok(record)
    }

    pub fn is_fresh(
        &self,
        context_snapshot_id: &str,
        project_id: &str,
        epoch: i64,
        state_digest: &str,
    ) -> Result<bool> {
        self.storage.validate_context_fresh(
            context_snapshot_id,
            project_id,
            epoch,
            state_digest,
        )
    }

    pub fn supersede(
        &self,
        context_snapshot_id: &str,
        at: &str,
    ) -> Result<()> {
        let record = self.storage.get_context_snapshot(context_snapshot_id)?
            .ok_or_else(|| mayasaba_storage::StorageError::NotFound(
                format!("context snapshot {context_snapshot_id}")
            ))?;
        self.storage.supersede_context_snapshot(context_snapshot_id, at)?;
        self.emit_event(
            &record,
            "CONTEXT_SUPERSEDED",
            &format!(
                r#"{{"context_snapshot_id":"{}","project_id":"{}","epoch":{},"state_digest":"{}"}}"#,
                record.context_snapshot_id, record.project_id, record.epoch, record.state_digest
            ),
            at,
        )
    }

    pub fn invalidate(
        &self,
        context_snapshot_id: &str,
        at: &str,
    ) -> Result<()> {
        let record = self.storage.get_context_snapshot(context_snapshot_id)?
            .ok_or_else(|| mayasaba_storage::StorageError::NotFound(
                format!("context snapshot {context_snapshot_id}")
            ))?;
        self.storage.invalidate_context_snapshot(context_snapshot_id, at)?;
        self.emit_event(
            &record,
            "CONTEXT_INVALIDATED",
            &format!(
                r#"{{"context_snapshot_id":"{}","project_id":"{}","epoch":{},"state_digest":"{}"}}"#,
                record.context_snapshot_id, record.project_id, record.epoch, record.state_digest
            ),
            at,
        )
    }

    fn emit_event(
        &self,
        snapshot: &ContextSnapshotRecord,
        event_type: &str,
        payload_json: &str,
        created_at: &str,
    ) -> Result<()> {
        let mut writer = self.storage.clone_for_event_write();
        writer.append_event(&NewEvent {
            event_id: format!("evt_ctx_{}_{}", snapshot.context_snapshot_id, event_type),
            project_id: Some(snapshot.project_id.clone()),
            session_id: None,
            event_type: event_type.to_owned(),
            correlation_id: Some(snapshot.context_snapshot_id.clone()),
            causation_id: None,
            epoch: Some(snapshot.epoch),
            payload_json: payload_json.to_owned(),
            created_at: created_at.to_owned(),
        })
    }
}
