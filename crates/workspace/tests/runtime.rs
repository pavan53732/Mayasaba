use mayasaba_storage::{NewAdmission, NewContextSnapshot, NewProject, Storage};

fn workspace_storage() -> Storage {
    let mut storage = Storage::open_in_memory().expect("storage");
    storage.create_project(&NewProject {
        project_id:"prj_ws_runtime".into(),
        local_path:r"C:\work\ws-runtime".into(),
        brief_id:"brief_ws_runtime".into(),
        brief_body:"workspace runtime".into(),
        brief_source:"TEST".into(),
        event_id:"evt_ws_runtime".into(),
        created_at:"1".into(),
    }).expect("project");
    storage.conn().execute(
        "INSERT INTO workspaces (workspace_id,project_id,kind,root_path,status,created_at)
         VALUES ('ws_runtime','prj_ws_runtime','AGENT','C:\\work\\ws-runtime','ACTIVE','1')", []
    ).expect("workspace");
    storage.conn().execute(
        "INSERT INTO tasks (task_id,project_id,objective,status,priority,risk,workspace_id,current_epoch,created_at,updated_at)
         VALUES ('task_runtime','prj_ws_runtime','work','READY',1,'LOW','ws_runtime',0,'1','1')", []
    ).expect("task");
    storage.conn().execute(
        "INSERT INTO context_snapshots (context_snapshot_id,project_id,epoch,scope,state_digest,pack_json,created_at)
         VALUES ('ctx_runtime','prj_ws_runtime',0,'TASK','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','{}','1')", []
    ).expect("context");
    storage
}

fn admitted(task_id:&str, admission_id:&str) -> NewAdmission {
    NewAdmission {
        admission_id:admission_id.into(),
        project_id:"prj_ws_runtime".into(),
        task_id:task_id.into(),
        workspace_id:"ws_runtime".into(),
        lease_id:None,
        agent_id:None,
        session_id:None,
        kind:"WORKSPACE_ADMISSION".into(),
        epoch:0,
        context_digest:Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        base_checkpoint_ref:None,
        changed_paths_json:"[]".into(),
        checks_json:r#"[{"check_id":"REPOSITORY_IDENTITY","status":"PASS"},{"check_id":"BASELINE_CLEAN","status":"PASS"},{"check_id":"WORKTREE_ASSIGNED","status":"PASS"},{"check_id":"PROTECTED_PATHS_DETERMINED","status":"PASS"},{"check_id":"CONTEXT_FRESH","status":"PASS"},{"check_id":"WORKSPACE_OWNERSHIP","status":"PASS"}]"#.into(),
        verdict:"ADMITTED".into(),
        refusal_reasons_json:None,
        supersedes_admission_id:None,
        created_at:"2".into(),
    }
}

#[test]
fn workspace_service_persists_admission_and_checkpoint() {
    let mut storage = workspace_storage();
    let admission = admitted("task_runtime","admit_runtime");
    let recorded = mayasaba_workspace::record_admission(&mut storage,&admission).expect("admission");
    assert_eq!(recorded.verdict,"ADMITTED");

    mayasaba_workspace::create_checkpoint(
        &storage,
        "cp_runtime",
        "prj_ws_runtime",
        "ws_runtime",
        Some("task_runtime"),
        None,
        None,
        0,
        "SAFE_POINT",
        Some("abc123"),
        Some("def456"),
        "3",
    ).expect("checkpoint");

    let row:String = storage.conn().query_row(
        "SELECT kind FROM workspace_checkpoints WHERE checkpoint_id='cp_runtime'", [], |r| r.get(0)
    ).expect("checkpoint row");
    assert_eq!(row,"SAFE_POINT");
}

#[test]
fn workspace_service_refuses_admission_that_claims_success_over_a_failed_check() {
    let mut storage = workspace_storage();
    let mut admission = admitted("task_runtime","admit_bad");
    admission.checks_json = r#"[{"check_id":"BASELINE_CLEAN","status":"FAIL"}]"#.into();
    assert!(mayasaba_workspace::record_admission(&mut storage,&admission).is_err());
}
