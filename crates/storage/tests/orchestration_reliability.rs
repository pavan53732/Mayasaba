//! Storage-level conformance for the long-running orchestration reliability refinement.

use mayasaba_storage::{NewProject, Storage};

fn project_storage() -> Storage {
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&NewProject {
            project_id: "prj_reliability".to_string(),
            local_path: "C:\\work\\reliability".to_string(),
            brief_id: "brf_reliability".to_string(),
            brief_body: "Reliability test".to_string(),
            brief_source: "TEST".to_string(),
            event_id: "evt_reliability".to_string(),
            created_at: "1700000000".to_string(),
        })
        .expect("project");
    storage
}

fn seed_task_lease_workspace(storage: &Storage) {
    storage.conn().execute(
        "INSERT INTO agents (agent_id, agent_type, executable, created_at, updated_at) VALUES ('agent_1','HERMES_AGENT','hermes','1','1')",
        [],
    ).expect("agent");
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id, project_id, kind, root_path, status, created_at) VALUES ('ws_1','prj_reliability','AGENT','C:\\work\\reliability','ACTIVE','1')",
        [],
    ).expect("workspace");
    storage.conn().execute(
        "INSERT INTO tasks (task_id, project_id, objective, status, priority, risk, workspace_id, current_epoch, created_at, updated_at) VALUES ('task_1','prj_reliability','test','READY',1,'LOW','ws_1',0,'1','1')",
        [],
    ).expect("task");
    storage.conn().execute(
        "INSERT INTO agent_sessions (session_id, project_id, agent_id, state, health_state, workspace_id, current_epoch, started_at) VALUES ('sess_1','prj_reliability','agent_1','READY','HEALTHY','ws_1',0,'1')",
        [],
    ).expect("session");
    storage.conn().execute(
        "INSERT INTO task_leases (lease_id, task_id, project_id, agent_id, session_id, workspace_id, lease_version, project_epoch, context_snapshot_id, state_digest, allowed_paths_json, required_capabilities_json, policy_scope, issued_at, heartbeat_at, expires_at, status) VALUES ('lease_1','task_1','prj_reliability','agent_1','sess_1','ws_1',7,0,'ctx_1',repeat('a',64),'[]','[]','PROJECT_WRITE','1','1','9999','ACTIVE')",
        [],
    ).expect("lease");
}

#[test]
fn reliability_tables_are_created_and_have_the_declared_guards() {
    let storage = Storage::open_in_memory().expect("open");
    for table in ["task_attempts", "resource_reservations", "workspace_revisions", "environment_snapshots", "certification_bindings"] {
        let exists: i64 = storage.conn().query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        ).expect("table lookup");
        assert_eq!(exists, 1, "{table} must be part of the canonical schema");
    }
}

#[test]
fn task_attempt_numbers_are_unique_and_invalid_states_are_rejected() {
    let storage = project_storage();
    seed_task_lease_workspace(&storage);
    storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, created_at) VALUES ('att_1','task_1','prj_reliability',1,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','STARTED','1')",
        [],
    ).expect("attempt");
    assert!(storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, created_at) VALUES ('att_2','task_1','prj_reliability',1,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','STARTED','2')",
        [],
    ).is_err(), "one task cannot have two attempt #1 records");
    assert!(storage.conn().execute(
        "INSERT INTO task_attempts (attempt_id, task_id, project_id, attempt_no, lease_id, agent_id, session_id, workspace_id, fence_token, project_epoch, context_snapshot_id, state, created_at) VALUES ('att_bad','task_1','prj_reliability',2,'lease_1','agent_1','sess_1','ws_1',7,0,'ctx_1','WORKING','3')",
        [],
    ).is_err(), "an undeclared attempt state must fail closed");
}

#[test]
fn exclusive_resources_are_globally_unique_while_held() {
    let storage = project_storage();
    seed_task_lease_workspace(&storage);
    storage.conn().execute(
        "INSERT INTO resource_reservations (reservation_id, project_id, task_id, lease_id, lease_version, resource_type, resource_key, mode, quantity, state, issued_at, expires_at, created_at) VALUES ('res_1','prj_reliability','task_1','lease_1',7,'PORT','3000','EXCLUSIVE',1,'HELD','1','9999','1')",
        [],
    ).expect("first reservation");
    assert!(storage.conn().execute(
        "INSERT INTO resource_reservations (reservation_id, project_id, task_id, lease_id, lease_version, resource_type, resource_key, mode, quantity, state, issued_at, expires_at, created_at) VALUES ('res_2','prj_reliability','task_1','lease_1',7,'PORT','3000','EXCLUSIVE',1,'HELD','2','9999','2')",
        [],
    ).is_err(), "an exclusive port cannot have two live owners");
}

#[test]
fn safe_provenance_records_accept_only_declared_vocabularies() {
    let storage = project_storage();
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id, project_id, kind, root_path, status, created_at) VALUES ('ws_2','prj_reliability','INTEGRATION','C:\\work\\reliability','ACTIVE','1')",
        [],
    ).expect("workspace");
    assert!(storage.conn().execute(
        "INSERT INTO workspace_revisions (revision_id, project_id, workspace_id, revision_no, source, status, observed_at, created_at) VALUES ('rev_1','prj_reliability','ws_2',1,'CONTROLLER','VERIFIED','1','1')",
        [],
    ).is_ok());
    assert!(storage.conn().execute(
        "INSERT INTO workspace_revisions (revision_id, project_id, workspace_id, revision_no, source, status, observed_at, created_at) VALUES ('rev_bad','prj_reliability','ws_2',2,'ROBOT','VERIFIED','2','2')",
        [],
    ).is_err(), "an undeclared revision source must fail closed");
}
