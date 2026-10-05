use mayasaba_core::ContextService;
use mayasaba_storage::{NewContextSnapshot, NewProject, Storage};

fn project() -> Storage {
    let mut storage = Storage::open_in_memory().expect("storage");
    storage.create_project(&NewProject {
        project_id: "prj_ctx".into(),
        local_path: r"C:\work\ctx".into(),
        brief_id: "brief_ctx".into(),
        brief_body: "context".into(),
        brief_source: "TEST".into(),
        event_id: "evt_ctx_genesis".into(),
        created_at: "1".into(),
    }).expect("project");
    storage
}

#[test]
fn context_snapshot_digest_is_deterministic_and_freshness_is_epoch_bound() {
    let storage = project();
    let mut service = ContextService::new(storage);
    let first = service.create_snapshot(&NewContextSnapshot {
        context_snapshot_id:"ctx_1".into(),
        project_id:"prj_ctx".into(),
        epoch:0,
        scope:"TASK".into(),
        pack_json:r#"{"project":"prj_ctx","objective":"build","rules":[]}"#.into(),
        created_at:"2".into(),
    }).expect("snapshot");

    assert_eq!(first.state_digest.len(),64);
    assert!(service.is_fresh("ctx_1","prj_ctx",0,&first.state_digest).expect("fresh"));
    assert!(!service.is_fresh("ctx_1","prj_ctx",1,&first.state_digest).expect("stale epoch"));

    service.storage().conn().execute(
        "UPDATE projects SET current_epoch=1, updated_at='3' WHERE project_id='prj_ctx'",[]
    ).expect("epoch");
    assert!(!service.is_fresh("ctx_1","prj_ctx",0,&first.state_digest).expect("epoch invalidates context"));
}

#[test]
fn context_lifecycle_is_event_atomic_and_terminal_marks_block_freshness() {
    let storage = project();
    let mut service = ContextService::new(storage);
    let snapshot = service.create_snapshot(&NewContextSnapshot {
        context_snapshot_id:"ctx_2".into(),
        project_id:"prj_ctx".into(),
        epoch:0,
        scope:"PROJECT".into(),
        pack_json:r#"{"objective":"build"}"#.into(),
        created_at:"2".into(),
    }).expect("snapshot");

    service.supersede("ctx_2","3").expect("supersede");
    assert!(!service.is_fresh("ctx_2","prj_ctx",0,&snapshot.state_digest).expect("superseded is stale"));

    let events: i64 = service.storage().conn().query_row(
        "SELECT COUNT(*) FROM events WHERE project_id='prj_ctx' AND event_type IN ('CONTEXT_CREATED','CONTEXT_SUPERSEDED')",
        [], |r| r.get(0)
    ).expect("events");
    assert_eq!(events,2);

    let mut storage = service.storage_mut();
    let fresh = storage.create_context_snapshot(&NewContextSnapshot {
        context_snapshot_id:"ctx_3".into(),
        project_id:"prj_ctx".into(),
        epoch:0,
        scope:"PROJECT".into(),
        pack_json:r#"{"objective":"new"}"#.into(),
        created_at:"4".into(),
    }).expect("new snapshot");
    drop(storage);

    service.invalidate("ctx_3","5").expect("invalidate");
    assert!(!service.is_fresh("ctx_3","prj_ctx",0,&fresh.state_digest).expect("invalidated is stale"));
}

#[test]
fn material_epoch_advance_invalidates_older_contexts_transactionally() {
    let storage = project();
    let mut service = ContextService::new(storage);
    let snapshot = service.create_snapshot(&NewContextSnapshot {
        context_snapshot_id:"ctx_epoch".into(),
        project_id:"prj_ctx".into(),
        epoch:0,
        scope:"PROJECT".into(),
        pack_json:r#"{"objective":"before"}"#.into(),
        created_at:"2".into(),
    }).expect("snapshot");

    let advanced = service.advance_epoch("prj_ctx","material requirement change","3")
        .expect("epoch advance");
    assert_eq!(advanced.previous_epoch,0);
    assert_eq!(advanced.new_epoch,1);
    assert_eq!(advanced.invalidated_contexts,1);
    assert!(!service.is_fresh("ctx_epoch","prj_ctx",0,&snapshot.state_digest).expect("stale"));

    let current: i64 = service.storage().conn().query_row(
        "SELECT current_epoch FROM projects WHERE project_id='prj_ctx'",[],|r| r.get(0)
    ).expect("epoch row");
    assert_eq!(current,1);
    let recorded: i64 = service.storage().conn().query_row(
        "SELECT epoch FROM project_epochs WHERE project_id='prj_ctx' AND epoch=1",[],|r| r.get(0)
    ).expect("epoch history");
    assert_eq!(recorded,1);
}
