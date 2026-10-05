use mayasaba_storage::{
    NewCommandExecution, NewEnvironmentSnapshot, NewProcessRecord, NewTaskAttempt, Storage,
};

fn project_storage() -> Storage {
    let mut storage = Storage::open_in_memory().expect("open");
    storage
        .create_project(&mayasaba_storage::NewProject {
            project_id: "prj_exec".into(),
            local_path: r"C:\work\exec".into(),
            brief_id: "brief_exec".into(),
            brief_body: "execution".into(),
            brief_source: "TEST".into(),
            event_id: "evt_exec".into(),
            created_at: "1".into(),
        })
        .expect("project");
    storage.conn().execute(
        "INSERT INTO agents (agent_id,agent_type,executable,created_at,updated_at)
         VALUES ('agent_exec','HERMES_AGENT','hermes','1','1')", [],
    ).expect("agent");
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id,project_id,kind,root_path,status,created_at)
         VALUES ('ws_exec','prj_exec','AGENT',?, 'ACTIVE','1')",
        [r"C:\work\exec"],
    ).expect("workspace");
    storage.conn().execute(
        "INSERT INTO tasks (task_id,project_id,objective,status,priority,risk,workspace_id,current_epoch,created_at,updated_at)
         VALUES ('task_exec','prj_exec','run','READY',1,'LOW','ws_exec',0,'1','1')",
        [],
    ).expect("task");
    storage.conn().execute(
        "INSERT INTO agent_sessions (session_id,project_id,agent_id,state,health_state,workspace_id,current_epoch,started_at)
         VALUES ('sess_exec','prj_exec','agent_exec','READY','HEALTHY','ws_exec',0,'1')", [],
    ).expect("session");
    storage.conn().execute(
        "INSERT INTO task_leases
         (lease_id,task_id,project_id,agent_id,session_id,workspace_id,lease_version,project_epoch,context_snapshot_id,state_digest,allowed_paths_json,required_capabilities_json,policy_scope,issued_at,heartbeat_at,expires_at,status)
         VALUES ('lease_exec','task_exec','prj_exec','agent_exec','sess_exec','ws_exec',3,0,'ctx_exec',
                 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','[]','[]','PROJECT_WRITE','1','1','9999','ACTIVE')",
        [],
    ).expect("lease");
    storage.insert_task_attempt(&NewTaskAttempt {
        attempt_id:"attempt_exec".into(), task_id:"task_exec".into(), project_id:"prj_exec".into(),
        attempt_no:1, lease_id:"lease_exec".into(), agent_id:"agent_exec".into(), session_id:"sess_exec".into(),
        workspace_id:"ws_exec".into(), fence_token:3, project_epoch:0, context_snapshot_id:"ctx_exec".into(),
        state:"RUNNING".into(), checkpoint_id:None, failure_id:None, started_at:Some("1".into()),
        heartbeat_at:Some("1".into()), ended_at:None, created_at:"1".into()
    }).expect("attempt");
    storage
}

fn command(status: &str, attempt_id: Option<&str>, env_id: Option<&str>) -> NewCommandExecution {
    NewCommandExecution {
        execution_id: format!("exec_{status}"),
        project_id: "prj_exec".into(),
        task_id: Some("task_exec".into()),
        attempt_id: attempt_id.map(str::to_owned),
        workspace_id: "ws_exec".into(),
        requested_by_agent_id: Some("agent_exec".into()),
        environment_snapshot_id: env_id.map(str::to_owned),
        supersedes_binding_id: None,
        classification: "PROJECT_WRITE".into(),
        executable: "cmd.exe".into(),
        arguments_json: r#"["/c","echo","ok"]"#.into(),
        cwd: r"C:\work\exec".into(),
        status: status.into(),
        exit_code: None,
        started_at: None,
        ended_at: None,
        timeout_seconds: 30,
        stdout_artifact_id: None,
        stderr_artifact_id: None,
    }
}

#[test]
fn command_execution_is_fenced_to_the_attempt_and_environment() {
    let storage = project_storage();
    storage.insert_environment_snapshot(&NewEnvironmentSnapshot {
        environment_snapshot_id:"env_exec".into(), project_id:"prj_exec".into(),
        workspace_id:Some("ws_exec".into()), task_id:Some("task_exec".into()),
        execution_id:None, os_identity:"Windows".into(), runtime_versions_json:"{}".into(),
        environment_policy_hash:"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        source:"EXECUTION".into(), captured_at:"1".into()
    }).expect("environment");
    storage.insert_command_execution(&command("REQUESTED", Some("attempt_exec"), Some("env_exec")))
        .expect("fenced execution");
    assert_eq!(storage.get_command_execution("exec_REQUESTED").expect("read").unwrap().attempt_id.as_deref(), Some("attempt_exec"));

    storage.conn().execute("UPDATE task_leases SET lease_version=4 WHERE lease_id='lease_exec'", []).expect("roll");
    assert!(storage.insert_command_execution(&NewCommandExecution {
        execution_id:"exec_stale".into(), ..command("REQUESTED", Some("attempt_exec"), Some("env_exec"))
    }).is_err(), "a stale attempt cannot create a new material execution");
}

#[test]
fn command_execution_rejects_bad_payload_and_illegal_transition() {
    let storage = project_storage();
    let mut bad = command("REQUESTED", None, None);
    bad.arguments_json = r#"{"not":"an array"}"#.into();
    assert!(storage.insert_command_execution(&bad).is_err());

    storage.insert_command_execution(&command("REQUESTED", None, None)).expect("insert");
    assert!(storage.transition_command_execution("exec_REQUESTED", "REQUESTED", "RUNNING", None, None).is_err());
    storage.transition_command_execution("exec_REQUESTED","REQUESTED","POLICY_CHECK",None,None).expect("policy check");
    storage.transition_command_execution("exec_REQUESTED","POLICY_CHECK","APPROVED",None,None).expect("approve");
    assert!(storage.transition_command_execution("exec_REQUESTED","POLICY_CHECK","DENIED",None,None).is_err(), "CAS must reject the stale expected state");
}

#[test]
fn process_observations_are_append_only_and_unknown_is_preserved() {
    let storage = project_storage();
    storage.insert_command_execution(&command("REQUESTED", None, None)).expect("execution");
    storage.insert_process_record(&NewProcessRecord {
        process_record_id:"proc_expected".into(), execution_id:"exec_REQUESTED".into(), pid:1001,
        parent_pid:Some(900), state:"EXPECTED".into(), observed_at:"2".into()
    }).expect("expected");
    storage.insert_process_record(&NewProcessRecord {
        process_record_id:"proc_unknown".into(), execution_id:"exec_REQUESTED".into(), pid:1001,
        parent_pid:Some(900), state:"UNKNOWN".into(), observed_at:"3".into()
    }).expect("unknown");
    let rows=storage.list_process_records("exec_REQUESTED").expect("rows");
    assert_eq!(rows.len(),2);
    assert_eq!(rows[0].state,"UNKNOWN");
    assert_eq!(storage.latest_process_record("exec_REQUESTED").expect("latest").unwrap().process_record_id,"proc_unknown");
    assert!(storage.insert_process_record(&NewProcessRecord {
        process_record_id:"proc_bad".into(), execution_id:"missing".into(), pid:1,
        parent_pid:None, state:"VERIFIED".into(), observed_at:"4".into()
    }).is_err());
}


#[test]
fn schedulable_selector_is_dependency_safe_and_deterministic() {
    let storage = project_storage();
    storage.conn().execute(
        "INSERT INTO tasks (task_id,project_id,objective,status,priority,risk,workspace_id,current_epoch,created_at,updated_at)
         VALUES
         ('task_low','prj_exec','low','READY',1,'LOW','ws_exec',0,'1','1'),
         ('task_high','prj_exec','high','READY',5,'LOW','ws_exec',0,'2','2'),
         ('task_blocked','prj_exec','blocked','READY',99,'LOW','ws_exec',0,'3','3')", [],
    ).expect("tasks");
    storage.conn().execute(
        "INSERT INTO task_dependencies (task_id, depends_on_task_id) VALUES ('task_blocked','task_high')", []
    ).expect("dependency");
    let rows = storage.list_schedulable_tasks("prj_exec", 10).expect("selector");
    assert_eq!(rows.iter().map(|r| r.task_id.as_str()).collect::<Vec<_>>(),
               vec!["task_high","task_low"]);
    assert!(!rows.iter().any(|r| r.task_id == "task_blocked"));

    storage.conn().execute("UPDATE tasks SET status='COMPLETED', updated_at='4' WHERE task_id='task_high'", [])
        .expect("complete dependency");
    let rows = storage.list_schedulable_tasks("prj_exec", 10).expect("selector after dependency");
    assert_eq!(rows.iter().map(|r| r.task_id.as_str()).collect::<Vec<_>>(),
               vec!["task_blocked","task_low"]);

    storage.conn().execute(
        "INSERT INTO task_leases (lease_id,task_id,project_id,agent_id,session_id,workspace_id,lease_version,project_epoch,context_snapshot_id,state_digest,allowed_paths_json,required_capabilities_json,policy_scope,issued_at,heartbeat_at,expires_at,status)
         VALUES ('lease_high','task_low','prj_exec','agent_exec','sess_exec','ws_exec',1,0,'ctx_exec',
                 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','[]','[]','PROJECT_WRITE','1','1','9999','ACTIVE')", []
    ).expect("live lease");
    let rows = storage.list_schedulable_tasks("prj_exec", 10).expect("selector with live lease");
    assert!(!rows.iter().any(|r| r.task_id == "task_low"));
}


#[test]
fn agent_session_lifecycle_is_cas_and_capability_snapshot_is_bound() {
    use mayasaba_storage::{NewAgentCapabilitySnapshot, NewAgentSession};
    let mut storage = project_storage();

    let session = storage.create_agent_session(&NewAgentSession {
        session_id:"sess_discovery".into(),
        project_id:"prj_exec".into(),
        agent_id:"agent_exec".into(),
        workspace_id:Some("ws_exec".into()),
        current_epoch:0,
        started_at:"10".into(),
    }).expect("session");
    assert_eq!(session.state, "DISCOVERED");
    assert_eq!(session.health_state, "UNKNOWN");

    storage.transition_agent_session(
        "sess_discovery","DISCOVERED","HANDSHAKING","AGENT_HANDSHAKING","11"
    ).expect("handshake");
    assert!(storage.transition_agent_session(
        "sess_discovery","DISCOVERED","CAPABILITY_VALIDATING","AGENT_CAPABILITY_VALIDATING","12"
    ).is_err(), "stale expected state must not overwrite session");

    storage.transition_agent_session(
        "sess_discovery","HANDSHAKING","CAPABILITY_VALIDATING","AGENT_CAPABILITY_VALIDATING","12"
    ).expect("capability gate");
    storage.insert_agent_capability_snapshot(&NewAgentCapabilitySnapshot {
        capability_snapshot_id:"cap_1".into(),
        agent_id:"agent_exec".into(),
        session_id:Some("sess_discovery".into()),
        capabilities_json:r#"{"version_probe":true,"structured_transport":true}"#.into(),
        detected_at:"12".into(),
    }).expect("capabilities");
    storage.transition_agent_session(
        "sess_discovery","CAPABILITY_VALIDATING","WORKSPACE_VALIDATING","AGENT_WORKSPACE_VALIDATING","13"
    ).expect("workspace gate");
}

#[test]
fn scheduler_refuses_ready_tasks_from_an_older_project_epoch() {
    let mut storage = project_storage();
    storage.conn().execute(
        "UPDATE projects SET current_epoch=1, updated_at='5' WHERE project_id='prj_exec'", []
    ).expect("epoch");
    let rows = storage.list_schedulable_tasks("prj_exec", 10).expect("selector");
    assert!(rows.iter().all(|r| r.task_id != "task_exec"), "stale task epoch must not be schedulable");
}


#[test]
fn child_agent_slot_budget_is_atomic_and_reuses_free_slots() {
    let mut storage = project_storage();
    storage.conn().execute(
        "INSERT INTO task_scopes (task_id,allowed_paths_json,required_capabilities_json,validation_requirements_json,policy_scope,max_attempts,max_parallel_children,created_at,updated_at)
         VALUES ('task_exec','[]','[]','[]','PROJECT_WRITE',3,2,'1','1')", []
    ).expect("scope");

    let first = storage.reserve_child_slot("slot_1","task_exec","lease_exec",3,"2","99")
        .expect("first slot");
    let second = storage.reserve_child_slot("slot_2","task_exec","lease_exec",3,"2","99")
        .expect("second slot");
    assert_eq!(first, "task:task_exec:child:1");
    assert_eq!(second, "task:task_exec:child:2");
    assert!(storage.reserve_child_slot("slot_3","task_exec","lease_exec",3,"2","99").is_err());

    storage.release_resource_reservation("slot_1",3,"3").expect("release first");
    let reused = storage.reserve_child_slot("slot_3","task_exec","lease_exec",3,"4","99")
        .expect("reuse first free slot");
    assert_eq!(reused, "task:task_exec:child:1");
}
