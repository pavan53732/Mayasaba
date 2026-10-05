//! Core recovery-plan tests for long-running task attempts and local resource expiry.

use mayasaba_core::{build_runtime_recovery_plan, RuntimeRecoveryAction};
use mayasaba_storage::{NewProject, NewResourceReservation, NewTaskAttempt, Storage};

fn storage() -> Storage {
    let mut s = Storage::open_in_memory().expect("open");
    s.create_project(&NewProject {
        project_id: "prj_recovery".into(),
        local_path: "C:\\work\\recovery".into(),
        brief_id: "brf_recovery".into(),
        brief_body: "recovery".into(),
        brief_source: "TEST".into(),
        event_id: "evt_recovery".into(),
        created_at: "1".into(),
    }).expect("project");
    s.conn().execute(
        "INSERT INTO agents (agent_id, agent_type, executable, created_at, updated_at) VALUES ('agent_1','HERMES_AGENT','hermes','1','1')", []
    ).expect("agent");
    s.conn().execute(
        "INSERT INTO workspaces (workspace_id, project_id, kind, root_path, status, created_at) VALUES ('ws_1','prj_recovery','AGENT','C:\\work\\recovery','ACTIVE','1')", []
    ).expect("workspace");
    s.conn().execute(
        "INSERT INTO tasks (task_id, project_id, objective, status, priority, risk, workspace_id, current_epoch, created_at, updated_at) VALUES ('task_1','prj_recovery','test','READY',1,'LOW','ws_1',0,'1','1')", []
    ).expect("task");
    s.conn().execute(
        "INSERT INTO agent_sessions (session_id, project_id, agent_id, state, health_state, workspace_id, current_epoch, started_at) VALUES ('sess_1','prj_recovery','agent_1','READY','HEALTHY','ws_1',0,'1')", []
    ).expect("session");
    s.conn().execute(
        "INSERT INTO task_leases (lease_id, task_id, project_id, agent_id, session_id, workspace_id, lease_version, project_epoch, context_snapshot_id, state_digest, allowed_paths_json, required_capabilities_json, policy_scope, issued_at, heartbeat_at, expires_at, status) VALUES ('lease_1','task_1','prj_recovery','agent_1','sess_1','ws_1',7,0,'ctx_1','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','[]','[]','PROJECT_WRITE','1','1','9999','ACTIVE')", []
    ).expect("lease");
    s
}

fn attempt(s: &Storage, state: &str) {
    s.insert_task_attempt(&NewTaskAttempt {
        attempt_id: format!("att_{state}"),
        task_id: "task_1".into(),
        project_id: "prj_recovery".into(),
        attempt_no: 1,
        lease_id: "lease_1".into(),
        agent_id: "agent_1".into(),
        session_id: "sess_1".into(),
        workspace_id: "ws_1".into(),
        fence_token: 7,
        project_epoch: 0,
        context_snapshot_id: "ctx_1".into(),
        state: state.into(),
        checkpoint_id: None,
        failure_id: None,
        started_at: Some("1".into()),
        heartbeat_at: Some("1".into()),
        ended_at: None,
        created_at: "1".into(),
    }).expect("attempt");
}

#[test]
fn recovery_plan_classifies_stale_attempts_and_expires_due_resources() {
    let s = storage();
    attempt(&s, "RUNNING");
    s.conn().execute(
        "UPDATE task_leases SET lease_version = 8 WHERE lease_id = 'lease_1'", []
    ).expect("roll lease");
    s.insert_resource_reservation(&NewResourceReservation {
        reservation_id: "res_1".into(),
        project_id: "prj_recovery".into(),
        task_id: "task_1".into(),
        lease_id: "lease_1".into(),
        lease_version: 8,
        resource_type: "PORT".into(),
        resource_key: "3000".into(),
        mode: "EXCLUSIVE".into(),
        quantity: 1,
        state: "HELD".into(),
        issued_at: "1".into(),
        expires_at: "5".into(),
        released_at: None,
        created_at: "1".into(),
    }).expect("reservation");
    let plan = build_runtime_recovery_plan(&s, "prj_recovery", "10").expect("plan");
    assert_eq!(plan.expired_resource_reservations, 1);
    assert_eq!(plan.candidates.len(), 1);
    assert_eq!(plan.candidates[0].action, RuntimeRecoveryAction::RecoverStaleFence);
    let state: String = s.conn().query_row(
        "SELECT state FROM resource_reservations WHERE reservation_id = 'res_1'", [], |r| r.get(0)
    ).expect("state");
    assert_eq!(state, "EXPIRED");
}

#[test]
fn unknown_attempts_require_inspection() {
    let s = storage();
    attempt(&s, "UNKNOWN");
    let plan = build_runtime_recovery_plan(&s, "prj_recovery", "1").expect("plan");
    assert_eq!(plan.candidates[0].action, RuntimeRecoveryAction::InspectUnknown);
}
